pub mod tool_execution;
pub use tool_execution::ToolExecutionContext;

use anyhow::Result;
use std::sync::Arc;
use futures_util::future::BoxFuture;
use crate::agent::{Agent, AgentContext, AgentResponse, ExecutionType, Task};
use crate::agent::llm_client::LlmClient;
use crate::agent::llm_types::LlmRequest;
use crate::agent::schema::{AgentRoles, ExecutionMode};
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

            // Clone tool_calls to avoid borrow checker issues across async boundaries
            if let Some(tool_calls) = response.tool_calls.clone() {
                self.persist_assistant_message(None, Some(&tool_calls))?;
                let tool_results = self.execute_tools(&tool_calls, pool).await?;

                for (tool_call, result) in tool_calls.iter().zip(tool_results.iter()) {
                    self.persist_tool_message(&tool_call.function.name, result)?;
                }

                // Add assistant + tool result turns for next LLM iteration
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
        match self.context.execution_type {
            ExecutionType::Agentic => self.execute_orchestrator(pool).await,
            ExecutionType::ToolProxy => self.execute_tool_proxy(pool).await,
        }
    }

    async fn execute_tool_proxy(&mut self, pool: &Arc<AgentPool>) -> Result<AgentResponse> {
        let mut messages = self.build_initial_messages();
        let user_goal = self.task.user_goal.clone();
        messages.push(Message {
            role: "user".to_string(),
            content: Some(user_goal),
            tool_calls: None,
        });

        let mut all_tool_results: Vec<(String, String)> = Vec::new();

        loop {
            self.update_system_prompt(&mut messages);
            let response = self.call_llm(&messages, pool).await?;

            if let Some(tool_calls) = response.tool_calls.clone() {
                self.persist_assistant_message(None, Some(&tool_calls))?;
                let tool_results = self.execute_tools(&tool_calls, pool).await?;

                for (tool_call, result) in tool_calls.iter().zip(tool_results.iter()) {
                    self.persist_tool_message(&tool_call.function.name, result)?;
                    all_tool_results.push((tool_call.function.name.clone(), result.clone()));
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

            // No tool calls — specialist is done reasoning
            break;
        }

        let xml = format_tool_results_xml(&all_tool_results);
        Ok(AgentResponse::complete(xml))
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
        // result here is already wrapped — store it as-is
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

fn format_tool_results_xml(results: &[(String, String)]) -> String {
    if results.is_empty() {
        return "<tool_results/>".to_string();
    }
    let inner: String = results
        .iter()
        .map(|(name, result)| format!("  <result name=\"{}\">{}</result>", name, result))
        .collect::<Vec<_>>()
        .join("\n");
    format!("<tool_results>\n{}\n</tool_results>", inner)
}
