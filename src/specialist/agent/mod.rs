mod attributes;

pub use attributes::{Strength, Capability};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use anyhow::Result;
use reqwest::Client;
use futures_util::StreamExt;
use std::io::{self, Write};

use crate::Message;
use crate::schema::{Tool, Task};

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
}

#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    pub message: ResponseMessage,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ResponseMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    pub function: FunctionCall,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Value,
}

impl ResponseMessage {
    pub fn to_message(&self) -> Message {
        Message {
            role: self.role.clone(),
            content: self.content.clone(),
            tool_calls: self.tool_calls.clone(),
        }
    }
}

#[derive(Deserialize, Debug)]
struct StreamChunk {
    message: ResponseMessage,
    done: bool,
}

pub struct Agent {
    pub strength: Strength,
    pub capability: Capability,
    client: Client,
}

impl Agent {
    pub fn new(strength: Strength, capability: Capability) -> Self {
        Self {
            strength,
            capability,
            client: Client::new(),
        }
    }

    fn ollama_url(&self) -> &'static str {
        self.strength.url()
    }

    fn model(&self) -> &'static str {
        match (&self.strength, &self.capability) {
            (Strength::Power, Capability::Reasoner) => "qwen3:32b",
            (Strength::Power, Capability::ToolCaller) => "qwen3:32b",
            (Strength::Power, Capability::Quick) => "qwen3:8b",
            (Strength::Power, Capability::Coder) => "qwen3:32b",
            (Strength::Speed, _) => "qwen3:8b",
        }
    }

    pub async fn make_request(&self, messages: &Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ResponseMessage> {
        let request = ChatRequest {
            model: self.model().to_string(),
            messages: messages.clone(),
            stream: false,
            tools,
        };
        let response = self
            .client
            .post(self.ollama_url())
            .json(&request)
            .send()
            .await?
            .json::<ChatResponse>()
            .await?;
        Ok(response.message)
    }

    pub async fn make_request_streaming(&self, messages: &Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ResponseMessage> {
        let request = ChatRequest {
            model: self.model().to_string(),
            messages: messages.clone(),
            stream: true,
            tools,
        };
        let response = self
            .client
            .post(self.ollama_url())
            .json(&request)
            .send()
            .await?;

        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut full_content = String::new();
        let mut tool_calls: Option<Vec<ToolCall>> = None;
        let mut role = String::from("assistant");

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            buffer.extend_from_slice(&bytes);

            while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=newline_pos).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let chunk: StreamChunk = serde_json::from_str(line)?;
                role = chunk.message.role.clone();

                if let Some(content) = &chunk.message.content {
                    if !content.is_empty() {
                        print!("{}", content);
                        io::stdout().flush()?;
                        full_content.push_str(content);
                    }
                }

                if chunk.done {
                    if chunk.message.tool_calls.is_some() {
                        tool_calls = chunk.message.tool_calls.clone();
                    }
                }
            }
        }

        if !buffer.is_empty() {
            let line = String::from_utf8_lossy(&buffer);
            let line = line.trim();
            if !line.is_empty() {
                let chunk: StreamChunk = serde_json::from_str(line)?;
                role = chunk.message.role.clone();
                if let Some(content) = &chunk.message.content {
                    if !content.is_empty() {
                        print!("{}", content);
                        io::stdout().flush()?;
                        full_content.push_str(content);
                    }
                }
                if chunk.done {
                    if chunk.message.tool_calls.is_some() {
                        tool_calls = chunk.message.tool_calls.clone();
                    }
                }
            }
        }

        Ok(ResponseMessage {
            role,
            content: if full_content.is_empty() { None } else { Some(full_content) },
            tool_calls,
        })
    }

    pub async fn create_title(&self, user_message: &Message) -> Result<String> {
        Ok(self.make_request(&vec![
            Message {
                role: "system".to_string(),
                content: Some(Task::TitleGeneration.instructions().to_string()),
                tool_calls: None,
            },
            user_message.clone()
        ], None)
            .await?
            .content
            .unwrap_or_else(|| "Untitled".to_string()))
    }

    pub async fn summarize(&self, text: &str) -> Result<String> {
        self.make_request(&vec![
            Message {
                role: "system".to_string(),
                content: Some(Task::Summarization.instructions().to_string()),
                tool_calls: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(text.to_string()),
                tool_calls: None,
            }
        ], None)
            .await?
            .content
            .ok_or_else(|| anyhow::anyhow!("No summary generated"))
    }
}
