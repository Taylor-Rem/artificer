use anyhow::Result;
use reqwest::Client;
use futures_util::StreamExt;

use crate::api::events::EventSender;
use crate::pool::GpuHandle;
use artificer_shared::{Tool, ToolCall, Message};

pub use schema::{Task, AgentContext, AgentResponse};

const MAX_ITERATIONS: u32 = 50;
const CONTEXT_PRUNE_THRESHOLD: usize = 40;

pub trait Agent {
    fn name(&self) -> &str;
    fn system_prompt(&self, memory_context: Option<&str>) -> String;
    fn available_tools(&self) -> Vec<Tool>;
    async fn dispatch(
        &self,
        tool_call: &ToolCall,
        task: &mut Task,
        context: &AgentContext,
    ) -> Result<String>;

    fn create_task(&self, goal: &str, context: &AgentContext) -> Result<Task>;

    // Default implementation - shared execution loop
    async fn execute(
        &self,
        goal: String,
        context: AgentContext,
        gpu: &GpuHandle,
        events: Option<&EventSender>,
        client: &Client,
    ) -> Result<AgentResponse> {
        // Create task
        let mut task = self.create_task(&goal, &context)?;

        // Load memory context for this agent
        let memory_context = self.load_memory_context(&context)?;

        // Build initial messages
        let system_prompt = self.system_prompt(memory_context.as_deref());
        let mut messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(system_prompt),
                tool_calls: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(goal.clone()),
                tool_calls: None,
            },
        ];

        let mut message_count = 0u32;
        let mut iterations = 0u32;

        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                let final_response = format!(
                    "Reached maximum iterations ({}) for this task. Current state:\n\n{}",
                    MAX_ITERATIONS,
                    task.state_summary()
                );
                context.db.fail_task(task.id)?;
                return Ok(AgentResponse::failed(final_response));
            }

            // Context pruning if needed
            if messages.len() > CONTEXT_PRUNE_THRESHOLD {
                messages = self.prune_context(&messages, &task, memory_context.as_deref());
            }

            // Call the model
            let response = call_model(
                gpu,
                &messages,
                &self.available_tools(),
                events,
                client,
            ).await?;

            // Persist assistant turn
            context.db.add_message(
                context.conversation_id,
                Some(task.id),
                "assistant",
                response.content.as_deref(),
                response.tool_calls.as_ref(),
                &mut message_count,
            )?;

            messages.push(Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
                tool_calls: response.tool_calls.clone(),
            });

            // Check for completion or tool calls
            match response.tool_calls {
                None => {
                    // No tool calls - natural completion
                    let content = response.content.unwrap_or_default();
                    context.db.complete_task(task.id)?;
                    return Ok(AgentResponse::complete(content));
                }

                Some(ref tool_calls) => {
                    // Execute each tool call
                    for tool_call in tool_calls {
                        let tool_name = &tool_call.function.name;
                        let args = &tool_call.function.arguments;

                        if let Some(ref ev) = context.events {
                            ev.tool_call(self.name(), tool_name, args.clone());
                        }

                        // Dispatch polymorphically
                        let result = self.dispatch(tool_call, &mut task, &context).await?;

                        if let Some(ref ev) = context.events {
                            ev.tool_result(self.name(), tool_name, result.clone());
                        }

                        // Persist tool result
                        context.db.add_message(
                            context.conversation_id,
                            Some(task.id),
                            "tool",
                            Some(&result),
                            None,
                            &mut message_count,
                        )?;

                        messages.push(Message {
                            role: "tool".to_string(),
                            content: Some(result),
                            tool_calls: None,
                        });

                        // Check if task was marked complete
                        if task.complete {
                            // Give model one final turn to write response
                            let final_response = call_model(
                                gpu,
                                &messages,
                                &self.available_tools(),
                                events,
                                client,
                            ).await?;

                            let content = final_response.content.unwrap_or_default();

                            context.db.add_message(
                                context.conversation_id,
                                Some(task.id),
                                "assistant",
                                Some(&content),
                                None,
                                &mut message_count,
                            )?;

                            context.db.complete_task(task.id)?;
                            return Ok(AgentResponse::complete(content));
                        }
                    }
                }
            }
        }
    }

    // Helper to load memory - can be overridden
    fn load_memory_context(&self, context: &AgentContext) -> Result<Option<String>> {
        // Default: load all memory for this device
        // Specialists can override to filter by specialist_id
        let raw = context.db.get_memory(context.device_id)?;
        let memories: Vec<serde_json::Value> = serde_json::from_str(&raw)?;
        if memories.is_empty() {
            return Ok(None);
        }

        let parts: Vec<String> = memories
            .iter()
            .filter_map(|m| {
                let key = m["key"].as_str()?;
                let value = m["value"].as_str()?;
                let mem_type = m["memory_type"].as_str().unwrap_or("fact");
                Some(format!("[{}] {}: {}", mem_type, key, value))
            })
            .collect();

        Ok(if parts.is_empty() { None } else { Some(parts.join("\n")) })
    }

    // Helper to prune context
    fn prune_context(
        &self,
        messages: &[Message],
        task: &Task,
        memory_context: Option<&str>,
    ) -> Vec<Message> {
        let system_prompt = self.system_prompt(memory_context);
        let mut pruned = vec![Message {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
        }];

        // Keep last 10 non-system messages for immediate context
        let recent: Vec<&Message> = messages
            .iter()
            .filter(|m| m.role != "system")
            .rev()
            .take(10)
            .collect();

        for msg in recent.into_iter().rev() {
            pruned.push(msg.clone());
        }

        pruned
    }
}

// Model calling helper
async fn call_model(
    gpu: &GpuHandle,
    messages: &[Message],
    tools: &[Tool],
    events: Option<&EventSender>,
    client: &Client,
) -> Result<ModelResponse> {
    let request_body = serde_json::json!({
        "model": gpu.model,
        "messages": messages,
        "stream": events.is_some(),
        "tools": if tools.is_empty() { None } else { Some(tools) },
    });

    if events.is_some() {
        call_model_streaming(gpu, request_body, events, client).await
    } else {
        call_model_standard(gpu, request_body, client).await
    }
}

async fn call_model_standard(
    gpu: &GpuHandle,
    request_body: serde_json::Value,
    client: &Client,
) -> Result<ModelResponse> {
    #[derive(serde::Deserialize)]
    struct OllamaResponse {
        message: OllamaMessage,
    }
    #[derive(serde::Deserialize)]
    struct OllamaMessage {
        content: Option<String>,
        tool_calls: Option<Vec<ToolCall>>,
    }

    let url = format!("{}/api/chat", gpu.url);
    let resp = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?
        .json::<OllamaResponse>()
        .await?;

    Ok(ModelResponse {
        content: resp.message.content,
        tool_calls: resp.message.tool_calls,
    })
}

async fn call_model_streaming(
    gpu: &GpuHandle,
    request_body: serde_json::Value,
    events: Option<&EventSender>,
    client: &Client,
) -> Result<ModelResponse> {
    #[derive(serde::Deserialize)]
    struct StreamChunk {
        message: StreamMessage,
    }
    #[derive(serde::Deserialize)]
    struct StreamMessage {
        content: Option<String>,
        tool_calls: Option<Vec<ToolCall>>,
    }

    let url = format!("{}/api/chat", gpu.url);
    let response = client.post(&url).json(&request_body).send().await?;

    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut full_content = String::new();
    let mut tool_calls: Option<Vec<ToolCall>> = None;

    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        buffer.extend_from_slice(&bytes);

        while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Ok(chunk) = serde_json::from_str::<StreamChunk>(line) {
                if let Some(content) = &chunk.message.content {
                    if !content.is_empty() {
                        if let Some(ref ev) = events {
                            ev.stream_chunk(content.clone());
                        }
                        full_content.push_str(content);
                    }
                }
                if chunk.message.tool_calls.is_some() {
                    tool_calls = chunk.message.tool_calls;
                }
            }
        }
    }

    Ok(ModelResponse {
        content: if full_content.is_empty() {
            None
        } else {
            Some(full_content)
        },
        tool_calls,
    })
}

#[derive(Debug, Clone)]
struct ModelResponse {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}