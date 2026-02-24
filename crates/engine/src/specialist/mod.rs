pub mod interactive;
pub mod background;

use anyhow::Result;
use reqwest::Client;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;

use crate::pool::{GpuHandle, GpuRole};
use crate::api::events::EventSender;
use artificer_shared::Tool;
use artificer_shared::tools as tool_registry;
use crate::Message;

// ============================================================================
// SPECIALIST TYPE
// ============================================================================

pub struct Specialist {
    pub name: &'static str,
    pub gpu_role: GpuRole,
    pub instructions: &'static str,
    pub toolbelts: &'static [&'static str],
}

// ============================================================================
// REGISTRY
// ============================================================================

pub static SPECIALISTS: &[Specialist] = &[
    interactive::WEB_RESEARCHER,
    interactive::FILE_SMITH,
    background::TITLE_GENERATION,
    background::SUMMARIZATION,
    background::MEMORY_EXTRACTION,
];

impl Specialist {
    pub fn all_interactive() -> impl Iterator<Item = &'static Specialist> {
        SPECIALISTS.iter().filter(|s| s.gpu_role == GpuRole::Interactive)
    }

    pub fn all_background() -> impl Iterator<Item = &'static Specialist> {
        SPECIALISTS.iter().filter(|s| s.gpu_role == GpuRole::Background)
    }

    pub fn find(name: &str) -> Option<&'static Specialist> {
        SPECIALISTS.iter().find(|s| s.name == name)
    }

    pub fn tools(&self) -> Vec<Tool> {
        tool_registry::get_tools_for(self.toolbelts)
    }
}

// ============================================================================
// EXECUTION
// ============================================================================

/// Response from a specialist execution
#[derive(Debug, Clone)]
pub struct SpecialistResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

pub use artificer_shared::{ToolCall, FunctionCall};

impl SpecialistResponse {
    pub fn to_message(&self) -> Message {
        Message {
            role: "assistant".to_string(),
            content: self.content.clone(),
            tool_calls: self.tool_calls.clone(),
        }
    }
}

impl Specialist {
    /// Execute the specialist on the given GPU.
    /// Handles both streaming and non-streaming based on whether events are provided.
    pub async fn execute(
        &self,
        gpu: &GpuHandle,
        messages: Vec<Message>,
        events: Option<&EventSender>,
        client: &Client,
    ) -> Result<SpecialistResponse> {
        let tools = self.tools();
        let tools_option = if tools.is_empty() { None } else { Some(tools) };

        let request_body = serde_json::json!({
            "model": gpu.model,
            "messages": messages,
            "stream": events.is_some(),
            "tools": tools_option,
        });

        if events.is_some() {
            execute_streaming(client, &gpu.url, request_body, events).await
        } else {
            execute_standard(client, &gpu.url, request_body).await
        }
    }

    /// Build the full message list for this specialist:
    /// system prompt (with injected memory if provided) + user messages
    pub fn build_messages(
        &self,
        user_messages: Vec<Message>,
        memory_context: Option<&str>,
    ) -> Vec<Message> {
        let mut system_content = self.instructions.to_string();

        // Inject tool schemas into system prompt so the specialist
        // knows what it has available
        let schemas = tool_registry::get_tool_schemas_for(self.toolbelts);
        if !schemas.is_empty() {
            system_content.push_str("\n\n# Available Tools\n");
            for schema in schemas {
                system_content.push_str(&format!("\n## {}\n{}\n", schema.name, schema.description));
                if !schema.parameters.is_empty() {
                    system_content.push_str("Parameters:\n");
                    for param in &schema.parameters {
                        system_content.push_str(&format!(
                            "- `{}` ({}{}): {}\n",
                            param.name,
                            param.type_name,
                            if param.required { ", required" } else { ", optional" },
                            param.description
                        ));
                    }
                }
            }
        }

        if let Some(memory) = memory_context {
            if !memory.is_empty() {
                system_content.push_str("\n\n# Context\n");
                system_content.push_str(memory);
            }
        }

        let mut messages = vec![Message {
            role: "system".to_string(),
            content: Some(system_content),
            tool_calls: None,
        }];
        messages.extend(user_messages);
        messages
    }
}

// ============================================================================
// HTTP EXECUTION HELPERS
// ============================================================================

async fn execute_standard(
    client: &Client,
    base_url: &str,
    request_body: Value,
) -> Result<SpecialistResponse> {
    #[derive(Deserialize)]
    struct OllamaResponse {
        message: OllamaMessage,
    }
    #[derive(Deserialize)]
    struct OllamaMessage {
        content: Option<String>,
        tool_calls: Option<Vec<ToolCall>>,
    }

    let url = format!("{}/api/chat", base_url);
    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?
        .json::<OllamaResponse>()
        .await?;

    Ok(SpecialistResponse {
        content: response.message.content,
        tool_calls: response.message.tool_calls,
    })
}

async fn execute_streaming(
    client: &Client,
    base_url: &str,
    request_body: Value,
    events: Option<&EventSender>,
) -> Result<SpecialistResponse> {
    #[derive(Deserialize)]
    struct StreamChunk {
        message: StreamMessage,
    }
    #[derive(Deserialize)]
    struct StreamMessage {
        content: Option<String>,
        tool_calls: Option<Vec<ToolCall>>,
    }

    let url = format!("{}/api/chat", base_url);
    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?;

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

            if let Ok(chunk) = serde_json::from_str::<StreamChunk>(line) {
                if let Some(content) = &chunk.message.content {
                    if !content.is_empty() {
                        if let Some(ev) = events {
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

    // Flush any remaining buffer
    if !buffer.is_empty() {
        let line = String::from_utf8_lossy(&buffer);
        let line = line.trim();
        if !line.is_empty() {
            if let Ok(chunk) = serde_json::from_str::<StreamChunk>(line) {
                if let Some(content) = &chunk.message.content {
                    if !content.is_empty() {
                        if let Some(ev) = events {
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

    Ok(SpecialistResponse {
        content: if full_content.is_empty() { None } else { Some(full_content) },
        tool_calls,
    })
}