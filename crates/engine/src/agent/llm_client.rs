use anyhow::Result;
use futures_util::StreamExt;
use reqwest::Client;
use crate::agent::llm_types::{LlmRequest, LlmResponse, StreamChunk};
use crate::pool::GpuHandle;
use crate::api::events::EventSender;
use artificer_shared::{Message, ToolCall};

pub struct LlmClient<'a> {
    client: &'a Client,
    gpu: &'a GpuHandle,
}

impl<'a> LlmClient<'a> {
    pub fn new(client: &'a Client, gpu: &'a GpuHandle) -> Self {
        Self { client, gpu }
    }

    /// Call LLM without streaming. Explicitly disables streaming.
    pub async fn call(&self, request: LlmRequest) -> Result<LlmResponse> {
        let request = request.with_streaming(false);
        let url = format!("{}/api/chat", self.gpu.url);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
            "LLM request failed ({}): {}",
            status,
            error_text
        ));
        }

        let llm_response: LlmResponse = response.json().await?;

        // ✓ Validate response has content
        if llm_response.message.content.is_none()
            && llm_response.message.tool_calls.is_none()
        {
            return Err(anyhow::anyhow!(
            "LLM returned empty response (no content and no tool_calls)"
        ));
        }

        Ok(llm_response)
    }

    /// Call LLM with streaming, emitting chunks via EventSender.
    pub async fn call_streaming(
        &self,
        request: LlmRequest,
        events: &EventSender,
    ) -> Result<Message> {
        let request = request.with_streaming(true);
        let url = format!("{}/api/chat", self.gpu.url);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
            "LLM streaming request failed ({}): {}",
            status,
            error_text
        ));
        }

        let mut stream = response.bytes_stream();
        let mut accumulated_content = String::new();
        let mut tool_calls: Option<Vec<ToolCall>> = None;
        let mut buffer = Vec::new();
        let mut done = false;  // ✓ Track done state at outer scope

        while let Some(chunk) = stream.next().await {
            if done {
                break;  // ✓ Exit stream consumption when done
            }

            let bytes = chunk?;
            buffer.extend_from_slice(&bytes);

            while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=newline_pos).collect();
                let line_str = String::from_utf8_lossy(&line);

                if line_str.trim().is_empty() {
                    continue;
                }

                if let Ok(chunk) = serde_json::from_str::<StreamChunk>(&line_str) {
                    if let Some(msg) = chunk.message {
                        if let Some(content) = msg.content {
                            if !content.is_empty() {
                                events.stream_chunk(content.clone());
                                accumulated_content.push_str(&content);
                            }
                        }
                        if let Some(calls) = msg.tool_calls {
                            tool_calls = Some(calls);
                        }
                    }

                    if chunk.done {
                        done = true;  // ✓ Set flag
                        break;        // ✓ Break inner loop
                    }
                }
            }
        }

        // ✓ Validate we got something back
        if accumulated_content.is_empty() && tool_calls.is_none() {
            return Err(anyhow::anyhow!(
            "LLM returned empty response (no content and no tool_calls)"
        ));
        }

        Ok(Message {
            role: "assistant".to_string(),
            content: if accumulated_content.is_empty() {
                None
            } else {
                Some(accumulated_content)
            },
            tool_calls,
        })
    }
}
