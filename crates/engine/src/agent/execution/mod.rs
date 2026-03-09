pub mod tool_execution;
pub use tool_execution::ToolExecutionContext;

pub mod specialist_state;
pub use specialist_state::SpecialistState;

use anyhow::Result;
use std::sync::Arc;
use futures_util::future::BoxFuture;
use crate::agent::{Agent, AgentContext, AgentResponse, Task};
use crate::agent::llm_client::LlmClient;
use crate::agent::llm_types::LlmRequest;
use crate::agent::schema::{AgentRoles, ExecutionMode};
use crate::agent::specialist_tools::{is_return_triggering_tool, is_specialist_control_tool, handle_specialist_control_tool};
use crate::agent::schema::task::{handle_task_tool, is_task_tool};
use crate::pool::AgentPool;
use artificer_shared::{Message, ToolCall};

#[cfg(test)]
mod tool_execution_tests;
pub mod tool_validation;

pub struct AgentExecution {
    agent: Agent,
    context: AgentContext,
    task: Task,
    agent_pool: Arc<AgentPool>,
    message_count: u32,
}

impl AgentExecution {
    pub fn new(
        agent: &Agent,
        context: AgentContext,
        goal: &str,
        pool: &Arc<AgentPool>,
    ) -> Self {
        let task = Task::new(
            &context,
            context.parent_task_id,
            goal,
            pool.db().clone(),
        );

        let message_count = pool
            .db()
            .get_message_count(context.conversation_id)
            .unwrap_or(0);

        Self {
            agent: agent.clone(),
            context,
            task,
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
        let user_goal = self.task.user_goal.clone();
        self.persist_user_message(&user_goal)?;
        messages.push(Message {
            role: "user".to_string(),
            content: Some(user_goal),
            tool_calls: None,
        });

        loop {
            self.update_system_prompt(&mut messages);
            let response = self.call_llm(&messages, pool).await?;

            if let Some(tool_calls) = response.tool_calls.clone() {
                self.persist_assistant_message(None, Some(&tool_calls))?;
                let tool_results = self.execute_tools(&tool_calls, pool).await?;

                for (tool_call, result) in tool_calls.iter().zip(tool_results.iter()) {
                    self.persist_tool_message(&tool_call.function.name, result)?;
                }

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

                if self.task.is_complete() {
                    break;
                }
                continue;
            }

            // Text response — stream already sent, persist and return
            if let Some(content) = &response.content {
                let content_owned = content.clone();
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

        let mut specialist_state = SpecialistState::new();
        let orchestrator_request = self.task.user_goal.clone();
        let mut iteration_count: u32 = 0;

        loop {
            iteration_count += 1;
            if iteration_count > MAX_SPECIALIST_ITERATIONS {
                eprintln!(
                    "Specialist hit max iteration limit ({}) for task {}",
                    MAX_SPECIALIST_ITERATIONS, self.task.id()
                );
                specialist_state.force_return();
                break;
            }

            let messages = self.build_specialist_messages(&specialist_state, &orchestrator_request);
            let response = self.call_llm(&messages, pool).await?;

            if let Some(tool_calls) = response.tool_calls.clone() {
                self.persist_assistant_message(None, Some(&tool_calls))?;

                // Separate return-triggering response tools from everything else
                let (return_calls, non_return_calls): (Vec<_>, Vec<_>) = tool_calls.iter()
                    .partition(|tc| is_return_triggering_tool(&tc.function.name));

                let (task_calls, regular_calls): (Vec<_>, Vec<_>) = non_return_calls.into_iter()
                    .partition(|tc| is_task_tool(&tc.function.name));

                let (response_control_calls, toolbelt_calls): (Vec<_>, Vec<_>) = regular_calls.into_iter()
                    .partition(|tc| is_specialist_control_tool(&tc.function.name));

                // Execute task management tools
                for tool_call in &task_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    if let Some(events) = &self.context.events {
                        events.tool_call(&format!("task_{}", self.task.id()), tool_name, args.clone());
                    }

                    let result = handle_task_tool(&mut self.task, tool_name, args)
                        .unwrap_or_else(|e| format!("Error: {}", e));

                    if let Some(events) = &self.context.events {
                        events.tool_result(&format!("task_{}", self.task.id()), tool_name, result.clone());
                    }
                    self.persist_tool_message(tool_name, &result)?;
                }

                // Execute regular toolbelt tools
                for tool_call in &toolbelt_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    if let Some(events) = &self.context.events {
                        events.tool_call(&format!("task_{}", self.task.id()), tool_name, args.clone());
                    }

                    let result = pool.tool_executor()
                        .execute(tool_name, args, self.context.device_id as i64, &self.context.device_key)
                        .await
                        .unwrap_or_else(|e| format!("Error: {}", e));

                    specialist_state.record_tool_call(tool_name.clone(), args.clone(), result.clone());

                    if let Some(events) = &self.context.events {
                        events.tool_result(&format!("task_{}", self.task.id()), tool_name, result.clone());
                    }
                    self.persist_tool_message(tool_name, &result)?;
                }

                // Execute response control tools (non-return: get_full_result, add_to_response)
                for tool_call in &response_control_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    if let Some(events) = &self.context.events {
                        events.tool_call(&format!("task_{}", self.task.id()), tool_name, args.clone());
                    }

                    let result = handle_specialist_control_tool(&mut specialist_state, tool_name, args)
                        .unwrap_or_else(|e| format!("Error: {}", e));

                    if let Some(events) = &self.context.events {
                        events.tool_result(&format!("task_{}", self.task.id()), tool_name, result.clone());
                    }
                    self.persist_tool_message(tool_name, &result)?;
                }

                // Only process return tools if they were the ONLY calls in this batch
                if !return_calls.is_empty() && task_calls.is_empty() && toolbelt_calls.is_empty() && response_control_calls.is_empty() {
                    for tool_call in &return_calls {
                        let tool_name = &tool_call.function.name;
                        let args = &tool_call.function.arguments;

                        if let Some(events) = &self.context.events {
                            events.tool_call(&format!("task_{}", self.task.id()), tool_name, args.clone());
                        }

                        let result = handle_specialist_control_tool(&mut specialist_state, tool_name, args)
                            .unwrap_or_else(|e| format!("Error: {}", e));

                        if let Some(events) = &self.context.events {
                            events.tool_result(&format!("task_{}", self.task.id()), tool_name, result.clone());
                        }
                        self.persist_tool_message(tool_name, &result)?;
                    }
                } else if !return_calls.is_empty() {
                    eprintln!(
                        "Warning: response:: return tools mixed with other tools in batch — ignoring return tools"
                    );
                }

                if specialist_state.should_return() || self.task.is_complete() {
                    break;
                }
                continue;
            }

            // Text-only response — treat as implicit return
            if let Some(content) = &response.content {
                let content_owned = content.clone();
                specialist_state.set_response_message(content_owned.clone());
                self.persist_assistant_message(Some(&content_owned), None)?;
                break;
            }

            return Err(anyhow::anyhow!("Specialist returned empty response"));
        }

        let final_message = specialist_state.response_message
            .clone()
            .unwrap_or_else(|| "Task completed.".to_string());

        let summary = specialist_state.build_delegation_summary(&final_message);
        Ok(AgentResponse::complete(summary))
    }

    fn build_specialist_messages(
        &self,
        specialist_state: &SpecialistState,
        orchestrator_request: &str,
    ) -> Vec<Message> {
        let system_prompt = self.agent.build_system_prompt("");

        let task_xml = self.task.state_summary_xml();
        let state_xml = specialist_state.build_state_xml(&task_xml);

        let user_content = format!(
            "<request>\n{}\n</request>\n\n<execution_state>\n{}\n</execution_state>",
            orchestrator_request,
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
                content: Some(self.task.user_goal.clone()),
                tool_calls: None,
            },
        ];

        let response = self.call_llm(&messages, pool).await?;

        if let Some(content) = response.content {
            self.task.mark_complete();
            Ok(AgentResponse::complete(content))
        } else {
            self.task.mark_failed(None);
            Err(anyhow::anyhow!("OneTime execution got no content"))
        }
    }

    fn build_initial_messages(&self) -> Vec<Message> {
        let system_message = Message {
            role: "system".to_string(),
            content: Some(self.build_system_prompt()),
            tool_calls: None,
        };

        // Orchestrator loads conversation history; specialists start fresh
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
        self.agent.build_system_prompt(&self.task.state_summary())
    }

    fn update_system_prompt(&self, messages: &mut Vec<Message>) {
        if let Some(first) = messages.first_mut() {
            if first.role == "system" {
                first.content = Some(self.build_system_prompt());
            }
        }
    }

    async fn call_llm(&self, messages: &[Message], pool: &Arc<AgentPool>) -> Result<Message> {
        let llm_client = LlmClient::new(pool.client(), self.task.gpu());
        let request = LlmRequest::new(self.task.gpu().model.clone(), messages.to_vec())
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
        let mut tool_ctx = ToolExecutionContext::new(&mut self.task, &self.context, pool);

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
            Some(self.task.id() as i64),
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
            Some(self.task.id() as i64),
            "assistant",
            content,
            tool_calls,
            &mut self.message_count,
        )
    }

    fn persist_tool_message(&mut self, _tool_name: &str, result: &str) -> Result<()> {
        self.agent_pool.db().add_message(
            self.context.conversation_id,
            Some(self.task.id() as i64),
            "tool",
            Some(result),
            None,
            &mut self.message_count,
        )
    }
}
