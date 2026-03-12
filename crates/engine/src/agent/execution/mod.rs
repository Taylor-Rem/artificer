pub mod tool_execution;
pub use tool_execution::ToolExecutionContext;

use anyhow::Result;
use std::sync::Arc;
use futures_util::future::BoxFuture;
use crate::agent::{Agent, AgentResponse};
use crate::agent::state::{TaskState, ExecutionContext, SpecialistExecution, AgentState};
use crate::agent::tools::{handle_task_tool, is_task_tool, handle_specialist_control_tool, is_return_triggering_tool};
use crate::agent::llm_client::LlmClient;
use crate::agent::llm_types::LlmRequest;
use crate::agent::{AgentRoles, ExecutionMode};
use crate::pool::AgentPool;
use artificer_shared::{Message, ToolCall};

#[cfg(test)]
mod tool_execution_tests;
pub mod tool_validation;

pub struct AgentExecution {
    agent: Agent,
    context: ExecutionContext,
    task_state: TaskState,
    agent_pool: Arc<AgentPool>,
    message_count: u32,
}

impl AgentExecution {
    pub fn new(
        agent: &Agent,
        context: ExecutionContext,
        goal: &str,
        pool: &Arc<AgentPool>,
    ) -> Self {
        let task_id = context.db
            .create_task(context.device_id, context.conversation_id, context.parent_task_id, goal)
            .expect("Failed to create task");

        let task_state = TaskState::new(task_id, context.parent_task_id, goal);

        let message_count = context.db
            .get_message_count(context.conversation_id)
            .unwrap_or(0);

        Self {
            agent: agent.clone(),
            context,
            task_state,
            agent_pool: pool.clone(),
            message_count,
        }
    }

    /// Execute the agent. Boxed to allow the recursive delegation call chain.
    pub fn execute(mut self, pool: Arc<AgentPool>) -> BoxFuture<'static, Result<AgentResponse>> {
        Box::pin(async move {
            match (self.agent.role, self.agent.execution_mode) {
                (AgentRoles::Orchestrator, ExecutionMode::Agentic) => {
                    self.execute_orchestrator(&pool).await
                }
                (AgentRoles::Specialist, ExecutionMode::Agentic) => {
                    self.execute_specialist(&pool).await
                }
                (AgentRoles::Background, ExecutionMode::OneTime) => {
                    self.execute_onetime(&pool).await
                }
                (AgentRoles::Background, ExecutionMode::Agentic) => {
                    self.execute_orchestrator(&pool).await
                }
                _ => Err(anyhow::anyhow!(
                    "Invalid agent configuration: {:?} with {:?}",
                    self.agent.role,
                    self.agent.execution_mode
                )),
            }
        })
    }

    async fn execute_orchestrator(&mut self, pool: &Arc<AgentPool>) -> Result<AgentResponse> {
        let mut messages = self.build_initial_messages();

        // Persist and add the current user message to the LLM context
        let user_goal = self.task_state.user_goal.clone();
        self.persist_user_message(&user_goal)?;
        messages.push(Message {
            role: "user".to_string(),
            content: Some(user_goal),
            tool_calls: None,
        });

        let mut iteration_count: u32 = 0;

        loop {
            iteration_count += 1;
            self.update_system_prompt(&mut messages);

            let system_preview = messages.first()
                .and_then(|m| m.content.as_deref())
                .map(|s| if s.len() > 500 { &s[..500] } else { s })
                .map(String::from);

            let input_context = messages.iter().rev()
                .find(|m| m.role == "user")
                .and_then(|m| m.content.as_deref())
                .unwrap_or("")
                .to_string();

            let start_time = std::time::Instant::now();
            let response = self.call_llm(&messages, pool).await?;
            let llm_duration = start_time.elapsed().as_millis() as u64;

            if let Some(tool_calls) = response.tool_calls.clone() {
                let reasoning = response.content.clone();

                if let (Some(text), Some(events)) = (&reasoning, &self.context.events) {
                    if !text.is_empty() {
                        events.reasoning(&format!("task_{}", self.task_state.id), text.clone());
                    }
                }

                self.persist_assistant_message(reasoning.as_deref(), Some(&tool_calls))?;
                let tool_results = self.execute_tools(&tool_calls, pool).await?;

                for (tool_call, result) in tool_calls.iter().zip(tool_results.iter()) {
                    self.persist_tool_message(&tool_call.function.name, result)?;
                }

                let classification = classify_orchestrator_iteration(&tool_calls);
                let tool_calls_json = serde_json::to_string(&tool_calls).ok();
                let tool_results_json = serde_json::to_string(&tool_results).ok();

                let _ = pool.db().log_execution_trace(
                    self.task_state.id,
                    self.agent.name,
                    iteration_count,
                    system_preview.as_deref(),
                    &input_context,
                    reasoning.as_deref(),
                    tool_calls_json.as_deref(),
                    tool_results_json.as_deref(),
                    &classification,
                    Some(llm_duration),
                );

                messages.push(Message {
                    role: "assistant".to_string(),
                    content: response.content.clone(),
                    tool_calls: Some(tool_calls),
                });
                for result in tool_results.iter() {
                    messages.push(Message {
                        role: "tool".to_string(),
                        content: Some(result.clone()),
                        tool_calls: None,
                    });
                }

                self.task_state.persist_if_dirty(&self.context)?;

                if self.task_state.is_complete() {
                    self.task_state.persist_complete(&self.context)?;
                    break;
                }
                continue;
            }

            // Text response — stream already sent, persist and return
            if let Some(content) = &response.content {
                let content_owned = content.clone();

                let _ = pool.db().log_execution_trace(
                    self.task_state.id,
                    self.agent.name,
                    iteration_count,
                    system_preview.as_deref(),
                    &input_context,
                    Some(&content_owned),
                    None,
                    None,
                    "text_response",
                    Some(llm_duration),
                );

                self.persist_assistant_message(Some(&content_owned), None)?;
                return Ok(AgentResponse::complete(content_owned));
            }

            return Err(anyhow::anyhow!("LLM returned empty response"));
        }

        // Task was marked complete via tool — generate a final summary
        let final_response = self.generate_final_response(&messages, pool).await?;
        self.persist_assistant_message(Some(&final_response), None)?;
        Ok(AgentResponse::complete(final_response))
    }

    async fn execute_specialist(&mut self, pool: &Arc<AgentPool>) -> Result<AgentResponse> {
        const MAX_SPECIALIST_ITERATIONS: u32 = 25;

        let mut specialist_exec = SpecialistExecution::new(self.task_state.clone());
        let mut iteration_count: u32 = 0;

        loop {
            iteration_count += 1;
            if iteration_count > MAX_SPECIALIST_ITERATIONS {
                eprintln!(
                    "Specialist hit max iteration limit ({}) for task {}",
                    MAX_SPECIALIST_ITERATIONS, specialist_exec.task.id
                );
                specialist_exec.force_return();
                break;
            }

            let messages = self.build_specialist_messages(&specialist_exec);

            let system_preview = messages.first()
                .and_then(|m| m.content.as_deref())
                .map(|s| if s.len() > 500 { &s[..500] } else { s })
                .map(String::from);

            let input_context = messages.last()
                .and_then(|m| m.content.as_deref())
                .unwrap_or("")
                .to_string();

            let start_time = std::time::Instant::now();
            let response = self.call_llm(&messages, pool).await?;
            let llm_duration = start_time.elapsed().as_millis() as u64;

            if let Some(tool_calls) = response.tool_calls.clone() {
                let reasoning = response.content.clone();

                if let (Some(text), Some(events)) = (&reasoning, &self.context.events) {
                    if !text.is_empty() {
                        events.reasoning(&format!("task_{}", specialist_exec.task.id), text.clone());
                    }
                }

                self.persist_assistant_message(reasoning.as_deref(), Some(&tool_calls))?;

                // Separate return-triggering response tools from everything else
                let (return_calls, non_return_calls): (Vec<_>, Vec<_>) = tool_calls.iter()
                    .partition(|tc| is_return_triggering_tool(&tc.function.name));

                let (task_calls, regular_calls): (Vec<_>, Vec<_>) = non_return_calls.into_iter()
                    .partition(|tc| is_task_tool(&tc.function.name));

                let (get_full_result_calls, toolbelt_calls): (Vec<_>, Vec<_>) = regular_calls.into_iter()
                    .partition(|tc| tc.function.name == "response::get_full_result");

                let mut tool_results_for_trace: Vec<String> = Vec::new();

                // Execute task management tools
                for tool_call in &task_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    if let Some(events) = &self.context.events {
                        events.tool_call(&format!("task_{}", specialist_exec.task.id), tool_name, args.clone());
                    }

                    let result = handle_task_tool(&mut specialist_exec.task, tool_name, args)
                        .unwrap_or_else(|e| format!("Error: {}", e));

                    if let Some(events) = &self.context.events {
                        events.tool_result(&format!("task_{}", specialist_exec.task.id), tool_name, result.clone());
                    }
                    tool_results_for_trace.push(result.clone());
                    self.persist_tool_message(tool_name, &result)?;
                }

                // Execute regular toolbelt tools
                for tool_call in &toolbelt_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    if let Some(events) = &self.context.events {
                        events.tool_call(&format!("task_{}", specialist_exec.task.id), tool_name, args.clone());
                    }

                    let result = pool.tool_executor()
                        .execute(tool_name, args, self.context.device_id as i64, &self.context.device_key)
                        .await
                        .unwrap_or_else(|e| format!("Error: {}", e));

                    specialist_exec.record_tool_call(tool_name.clone(), args.clone(), result.clone());

                    if let Some(events) = &self.context.events {
                        events.tool_result(&format!("task_{}", specialist_exec.task.id), tool_name, result.clone());
                    }
                    tool_results_for_trace.push(result.clone());
                    self.persist_tool_message(tool_name, &result)?;
                }

                // Execute response::get_full_result (read-only, not return-triggering)
                for tool_call in &get_full_result_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    if let Some(events) = &self.context.events {
                        events.tool_call(&format!("task_{}", specialist_exec.task.id), tool_name, args.clone());
                    }

                    let result = handle_specialist_control_tool(&mut specialist_exec, tool_name, args)
                        .unwrap_or_else(|e| format!("Error: {}", e));

                    if let Some(events) = &self.context.events {
                        events.tool_result(&format!("task_{}", specialist_exec.task.id), tool_name, result.clone());
                    }
                    tool_results_for_trace.push(result.clone());
                    self.persist_tool_message(tool_name, &result)?;
                }

                // Only process return tools if they were the ONLY calls in this batch
                if !return_calls.is_empty() && task_calls.is_empty() && toolbelt_calls.is_empty() && get_full_result_calls.is_empty() {
                    for tool_call in &return_calls {
                        let tool_name = &tool_call.function.name;
                        let args = &tool_call.function.arguments;

                        if let Some(events) = &self.context.events {
                            events.tool_call(&format!("task_{}", specialist_exec.task.id), tool_name, args.clone());
                        }

                        let result = handle_specialist_control_tool(&mut specialist_exec, tool_name, args)
                            .unwrap_or_else(|e| format!("Error: {}", e));

                        if let Some(events) = &self.context.events {
                            events.tool_result(&format!("task_{}", specialist_exec.task.id), tool_name, result.clone());
                        }
                        tool_results_for_trace.push(result.clone());
                        self.persist_tool_message(tool_name, &result)?;
                    }
                } else if !return_calls.is_empty() {
                    eprintln!(
                        "Warning: response:: return tools mixed with other tools in batch — ignoring return tools"
                    );
                }

                let classification = classify_specialist_iteration(&tool_calls);
                let tool_calls_json = serde_json::to_string(&tool_calls).ok();
                let tool_results_json = serde_json::to_string(&tool_results_for_trace).ok();

                let _ = pool.db().log_execution_trace(
                    specialist_exec.task.id,
                    self.agent.name,
                    iteration_count,
                    system_preview.as_deref(),
                    &input_context,
                    reasoning.as_deref(),
                    tool_calls_json.as_deref(),
                    tool_results_json.as_deref(),
                    &classification,
                    Some(llm_duration),
                );

                if specialist_exec.should_terminate() {
                    break;
                }
                continue;
            }

            // Text-only response — treat as implicit return
            if let Some(content) = &response.content {
                let content_owned = content.clone();

                let _ = pool.db().log_execution_trace(
                    specialist_exec.task.id,
                    self.agent.name,
                    iteration_count,
                    system_preview.as_deref(),
                    &input_context,
                    Some(&content_owned),
                    None,
                    None,
                    "text_only",
                    Some(llm_duration),
                );

                specialist_exec.set_response_message(content_owned.clone());
                self.persist_assistant_message(Some(&content_owned), None)?;
                break;
            }

            return Err(anyhow::anyhow!("Specialist returned empty response"));
        }

        let summary = specialist_exec.build_response();
        specialist_exec.task.persist_if_dirty(&self.context)?;
        Ok(AgentResponse::complete(summary))
    }

    fn build_specialist_messages(&self, specialist_exec: &SpecialistExecution) -> Vec<Message> {
        let system_prompt = self.agent.build_system_prompt("");
        let state_xml = specialist_exec.build_state_xml();

        let user_content = format!(
            "<request>\n{}\n</request>\n\n<execution_state>\n{}\n</execution_state>",
            specialist_exec.task.user_goal,
            state_xml
        );

        vec![
            Message {
                role: "system".to_string(),
                content: Some(system_prompt),
                tool_calls: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(user_content),
                tool_calls: None,
            },
        ]
    }

    async fn execute_onetime(&mut self, pool: &Arc<AgentPool>) -> Result<AgentResponse> {
        let messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(self.agent.system_prompt.to_string()),
                tool_calls: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(self.task_state.user_goal.clone()),
                tool_calls: None,
            },
        ];

        let response = self.call_llm(&messages, pool).await?;

        if let Some(content) = response.content {
            self.task_state.mark_complete();
            self.task_state.persist_complete(&self.context)?;
            Ok(AgentResponse::complete(content))
        } else {
            self.task_state.mark_failed();
            self.task_state.persist_failed(&self.context)?;
            Err(anyhow::anyhow!("OneTime execution got no content"))
        }
    }

    fn build_initial_messages(&self) -> Vec<Message> {
        let system_message = Message {
            role: "system".to_string(),
            content: Some(self.build_system_prompt()),
            tool_calls: None,
        };

        if self.agent.role == AgentRoles::Orchestrator {
            let history = self
                .agent_pool
                .db()
                .get_messages(self.context.conversation_id)
                .unwrap_or_default();

            let mut messages = vec![system_message];
            messages.extend(history);
            messages
        } else {
            vec![system_message]
        }
    }

    fn build_system_prompt(&self) -> String {
        self.agent.build_system_prompt(&self.task_state.build_task_xml())
    }

    fn update_system_prompt(&self, messages: &mut Vec<Message>) {
        if let Some(first) = messages.first_mut() {
            if first.role == "system" {
                first.content = Some(self.build_system_prompt());
            }
        }
    }

    async fn call_llm(&self, messages: &[Message], pool: &Arc<AgentPool>) -> Result<Message> {
        let llm_client = LlmClient::new(pool.client(), &self.context.gpu);
        let request = LlmRequest::new(self.context.gpu.model.clone(), messages.to_vec())
            .with_tools(self.agent.tools.clone());

        if let Some(events) = &self.context.events {
            llm_client.call_streaming(request, events).await
        } else {
            let response = llm_client.call(request).await?;
            Ok(response.message)
        }
    }

    async fn execute_tools(
        &mut self,
        tool_calls: &[ToolCall],
        pool: &Arc<AgentPool>,
    ) -> Result<Vec<String>> {
        let mut results = Vec::new();
        let mut tool_ctx = ToolExecutionContext::new(&mut self.task_state, &self.context, pool);

        for tool_call in tool_calls {
            let result = tool_ctx
                .execute_tool(&tool_call.function.name, &tool_call.function.arguments)
                .await?;

            let wrapped = format!(
                "<tool_response>\n<tool_name>{}</tool_name>\n<tool_result>\n{}\n</tool_result>\n</tool_response>",
                tool_call.function.name,
                result,
            );
            results.push(wrapped);
        }

        Ok(results)
    }

    async fn generate_final_response(
        &self,
        messages: &[Message],
        pool: &Arc<AgentPool>,
    ) -> Result<String> {
        let mut final_messages = messages.to_vec();
        final_messages.push(Message {
            role: "user".to_string(),
            content: Some(
                "The task is complete. Summarize what was accomplished for the user.".to_string(),
            ),
            tool_calls: None,
        });

        let response = self.call_llm(&final_messages, pool).await?;
        response
            .content
            .ok_or_else(|| anyhow::anyhow!("No final response generated"))
    }

    fn persist_user_message(&mut self, content: &str) -> Result<()> {
        self.agent_pool.db().add_message(
            self.context.conversation_id,
            Some(self.task_state.id as i64),
            "user",
            Some(content),
            None,
            &mut self.message_count,
        )
    }

    fn persist_assistant_message(
        &mut self,
        content: Option<&str>,
        tool_calls: Option<&Vec<ToolCall>>,
    ) -> Result<()> {
        self.agent_pool.db().add_message(
            self.context.conversation_id,
            Some(self.task_state.id as i64),
            "assistant",
            content,
            tool_calls,
            &mut self.message_count,
        )
    }

    fn persist_tool_message(&mut self, _tool_name: &str, result: &str) -> Result<()> {
        self.agent_pool.db().add_message(
            self.context.conversation_id,
            Some(self.task_state.id as i64),
            "tool",
            Some(result),
            None,
            &mut self.message_count,
        )
    }
}

fn classify_specialist_iteration(tool_calls: &[ToolCall]) -> String {
    let has_return_with = tool_calls.iter().any(|tc| tc.function.name == "response::return_with_tool_call");
    let has_return_as_is = tool_calls.iter().any(|tc| tc.function.name == "response::return_as_is");
    let has_add = tool_calls.iter().any(|tc| tc.function.name == "response::add_to_response");
    let has_get_full = tool_calls.iter().any(|tc| tc.function.name == "response::get_full_result");
    let has_task = tool_calls.iter().any(|tc| tc.function.name.starts_with("task::"));
    let has_toolbelt = tool_calls.iter().any(|tc| {
        !tc.function.name.starts_with("response::") && !tc.function.name.starts_with("task::")
    });

    if (has_return_with || has_return_as_is) && has_toolbelt {
        "mixed_return_ignored".to_string()
    } else if has_return_with {
        "return_with_tool_call".to_string()
    } else if has_return_as_is {
        "return_as_is".to_string()
    } else if has_add {
        "add_to_response".to_string()
    } else if has_get_full {
        "get_full_result".to_string()
    } else if has_toolbelt {
        "tool_call".to_string()
    } else if has_task {
        "task_management".to_string()
    } else {
        "unknown".to_string()
    }
}

fn classify_orchestrator_iteration(tool_calls: &[ToolCall]) -> String {
    let has_delegation = tool_calls.iter().any(|tc| tc.function.name.starts_with("delegate::"));
    let has_mark_complete = tool_calls.iter().any(|tc| tc.function.name == "task::mark_complete");
    let has_task = tool_calls.iter().any(|tc| tc.function.name.starts_with("task::"));

    if has_delegation {
        "delegation".to_string()
    } else if has_mark_complete {
        "mark_complete".to_string()
    } else if has_task {
        "task_management".to_string()
    } else {
        "tool_call".to_string()
    }
}
