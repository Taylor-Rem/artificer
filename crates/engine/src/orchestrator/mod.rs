pub mod task;
pub mod system_prompt;
pub mod tools;

use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;
use reqwest::Client;
use futures_util::StreamExt;

use artificer_shared::db::Db;
use crate::api::events::EventSender;
use crate::pool::GpuHandle;
use crate::ToolCall;
use crate::Message;

use task::Task;

const MAX_ITERATIONS: u32 = 50;
const CONTEXT_PRUNE_THRESHOLD: usize = 40;

#[derive(Debug, Clone)]
struct OrchestratorResponse {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}

pub struct Orchestrator {
    db: Arc<Db>,
    gpu: GpuHandle,
    device_id: i64,
    events: Option<EventSender>,
    client: Client,
}

impl Orchestrator {
    pub fn new(
        db: Arc<Db>,
        gpu: GpuHandle,
        device_id: i64,
        events: Option<EventSender>,
    ) -> Self {
        Self { db, gpu, device_id, events, client: Client::new() }
    }

    pub async fn run(
        &self,
        goal: String,
        conversation_id: u64,
        history: Vec<Message>,
        mut message_count: u32,
    ) -> Result<String> {
        let task_id = self.db.create_task(self.device_id, conversation_id, &goal)?;
        let mut task = Task::new(goal.clone(), task_id);
        let memory_context = self.load_memory_context()?;

        let system_prompt = system_prompt::build(None, memory_context.as_deref());
        let mut messages = self.build_initial_messages(system_prompt, history, &goal);

        let mut iterations = 0u32;

        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                let final_response = format!(
                    "I've reached the maximum number of steps for this task. \
                    Here is what I was able to accomplish:\n\n{}",
                    task.state_summary()
                );
                self.db.fail_task(task_id)?;
                self.db.queue_task_jobs(self.device_id, task_id)?;
                return Ok(final_response);
            }

            if messages.len() > CONTEXT_PRUNE_THRESHOLD {
                messages = self.prune_context(&messages, &task, memory_context.as_deref());
            }

            let response = self.call_model(&messages).await?;

            // Persist assistant turn before doing anything with it
            self.db.add_message(
                conversation_id,
                Some(task_id),
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

            match response.tool_calls {
                None => {
                    // No tool calls — natural completion signal.
                    let content = response.content.unwrap_or_default();
                    self.db.complete_task(task_id)?;
                    self.db.queue_task_jobs(self.device_id, task_id)?;
                    return Ok(content);
                }

                Some(ref tool_calls) => {
                    for tool_call in tool_calls {
                        let tool_name = &tool_call.function.name;
                        let args = &tool_call.function.arguments;

                        if let Some(ref ev) = self.events {
                            ev.tool_call("orchestrator", tool_name, args.clone());
                        }

                        let result = tools::handle(
                            tool_name,
                            args,
                            &mut task,
                            &self.db,
                            &self.gpu,
                            self.events.as_ref(),
                            &self.client,
                        ).await?;

                        if let Some(ref ev) = self.events {
                            ev.tool_result("orchestrator", tool_name, result.clone());
                        }

                        self.db.add_message(
                            conversation_id,
                            Some(task_id),
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

                        if task.complete {
                            // Give the model one final turn to write the response
                            // with full awareness that the task is done
                            let final_response = self.call_model(&messages).await?;
                            let content = final_response.content.unwrap_or_default();

                            self.db.add_message(
                                conversation_id,
                                Some(task_id),
                                "assistant",
                                Some(&content),
                                None,
                                &mut message_count,
                            )?;

                            self.db.complete_task(task_id)?;
                            self.db.queue_task_jobs(self.device_id, task_id)?;
                            return Ok(content);
                        }
                    }
                }
            }
        }
    }

    fn build_initial_messages(
        &self,
        system_prompt: String,
        history: Vec<Message>,
        goal: &str,
    ) -> Vec<Message> {
        let mut messages = vec![Message {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
        }];
        messages.extend(history);
        messages.push(Message {
            role: "user".to_string(),
            content: Some(goal.to_string()),
            tool_calls: None,
        });
        messages
    }

    fn prune_context(
        &self,
        messages: &[Message],
        task: &Task,
        memory_context: Option<&str>,
    ) -> Vec<Message> {
        let system_prompt = system_prompt::build(Some(task), memory_context);
        let mut pruned = vec![Message {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
        }];

        // Retain the last 10 non-system messages so the model has immediate context
        let recent: Vec<&Message> = messages.iter()
            .filter(|m| m.role != "system")
            .rev()
            .take(10)
            .collect();

        for msg in recent.into_iter().rev() {
            pruned.push(msg.clone());
        }

        pruned
    }

    async fn call_model(&self, messages: &[Message]) -> Result<OrchestratorResponse> {
        let request_body = serde_json::json!({
            "model": self.gpu.model,
            "messages": messages,
            "stream": self.events.is_some(),
            "tools": tools::definitions(),
        });

        if self.events.is_some() {
            self.call_model_streaming(request_body).await
        } else {
            self.call_model_standard(request_body).await
        }
    }

    async fn call_model_standard(&self, request_body: Value) -> Result<OrchestratorResponse> {
        #[derive(serde::Deserialize)]
        struct OllamaResponse { message: OllamaMessage }
        #[derive(serde::Deserialize)]
        struct OllamaMessage {
            content: Option<String>,
            tool_calls: Option<Vec<ToolCall>>,
        }

        let url = format!("{}/api/chat", self.gpu.url);
        let resp = self.client.post(&url).json(&request_body).send().await?
            .json::<OllamaResponse>().await?;

        Ok(OrchestratorResponse {
            content: resp.message.content,
            tool_calls: resp.message.tool_calls,
        })
    }

    async fn call_model_streaming(&self, request_body: Value) -> Result<OrchestratorResponse> {
        let url = format!("{}/api/chat", self.gpu.url);
        let response = self.client.post(&url).json(&request_body).send().await?;

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
                if line.is_empty() { continue; }

                #[derive(serde::Deserialize)]
                struct StreamChunk { message: StreamMessage }
                #[derive(serde::Deserialize)]
                struct StreamMessage {
                    content: Option<String>,
                    tool_calls: Option<Vec<ToolCall>>,
                }

                if let Ok(chunk) = serde_json::from_str::<StreamChunk>(line) {
                    if let Some(content) = &chunk.message.content {
                        if !content.is_empty() {
                            if let Some(ref ev) = self.events {
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

        Ok(OrchestratorResponse {
            content: if full_content.is_empty() { None } else { Some(full_content) },
            tool_calls,
        })
    }

    fn load_memory_context(&self) -> Result<Option<String>> {
        let raw = self.db.get_memory(self.device_id)?;
        let memories: Vec<Value> = serde_json::from_str(&raw)?;
        if memories.is_empty() { return Ok(None); }

        let parts: Vec<String> = memories.iter()
            .filter_map(|m| {
                let key = m["key"].as_str()?;
                let value = m["value"].as_str()?;
                let mem_type = m["memory_type"].as_str().unwrap_or("fact");
                Some(format!("[{}] {}: {}", mem_type, key, value))
            })
            .collect();

        Ok(if parts.is_empty() { None } else { Some(parts.join("\n")) })
    }
}