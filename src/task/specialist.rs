use serde::{Deserialize, Serialize};
use serde_json::Value;
use anyhow::Result;
use reqwest::Client;
use futures_util::StreamExt;
use std::io::{self, Write};

use crate::Message;
use crate::tools::Tool;

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

pub enum Specialist {
    PowerToolCaller,
    PowerReasoner,
    PowerQuick,
    PowerCoder,
    SpeedToolCaller,
    SpeedReasoner,
    SpeedQuick,
    SpeedCoder,
}

impl Specialist {
    pub fn url(&self) -> &'static str {
        match self {
            Specialist::PowerToolCaller
            | Specialist::PowerReasoner
            | Specialist::PowerQuick
            | Specialist::PowerCoder => "http://localhost:11435/api/chat",

            Specialist::SpeedToolCaller
            | Specialist::SpeedReasoner
            | Specialist::SpeedQuick
            | Specialist::SpeedCoder => "http://localhost:11434/api/chat",
        }
    }

    pub fn model(&self) -> &'static str {
        match self {
            Specialist::PowerToolCaller => "qwen3:32b",
            Specialist::PowerReasoner => "qwen3:32b",
            Specialist::PowerQuick => "qwen3:8b",
            Specialist::PowerCoder => "qwen3:32b",

            Specialist::SpeedToolCaller => "qwen3:8b",
            Specialist::SpeedReasoner => "qwen3:8b",
            Specialist::SpeedQuick => "qwen3:8b",
            Specialist::SpeedCoder => "qwen3:8b",
        }
    }

    pub fn tools(&self) -> Vec<Tool> {
        use crate::tools::registry;

        match self {
            // Full toolbelt for chat/research
            Specialist::PowerToolCaller => registry::get_tools(),

            // Task selector gets only the task selection tool
            Specialist::PowerQuick => registry::get_tools_for(&["TaskSelector::select_task"]),

            // No tools for pure reasoning/generation tasks
            Specialist::PowerReasoner
            | Specialist::SpeedReasoner
            | Specialist::SpeedQuick => vec![],

            // Coder gets file manipulation tools only
            Specialist::PowerCoder
            | Specialist::SpeedCoder => registry::get_tools_for(&["FileSmith"]),

            Specialist::SpeedToolCaller => registry::get_tools(),
        }
    }

    pub async fn execute(
        &self,
        messages: Vec<Message>,
        streaming: bool,
    ) -> Result<ResponseMessage> {
        let tools = self.tools();
        let tools_option = if tools.is_empty() { None } else { Some(tools) };

        if streaming {
            self.execute_streaming(messages, tools_option).await
        } else {
            self.execute_standard(messages, tools_option).await
        }
    }

    async fn execute_standard(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ResponseMessage> {
        let client = Client::new();
        let request = ChatRequest {
            model: self.model().to_string(),
            messages,
            stream: false,
            tools,
        };

        let response = client
            .post(self.url())
            .json(&request)
            .send()
            .await?
            .json::<ChatResponse>()
            .await?;

        Ok(response.message)
    }

    async fn execute_streaming(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
    ) -> Result<ResponseMessage> {
        let client = Client::new();
        let request = ChatRequest {
            model: self.model().to_string(),
            messages,
            stream: true,
            tools,
        };

        let response = client
            .post(self.url())
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

                if chunk.done && chunk.message.tool_calls.is_some() {
                    tool_calls = chunk.message.tool_calls.clone();
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
                if chunk.done && chunk.message.tool_calls.is_some() {
                    tool_calls = chunk.message.tool_calls.clone();
                }
            }
        }

        Ok(ResponseMessage {
            role,
            content: if full_content.is_empty() { None } else { Some(full_content) },
            tool_calls,
        })
    }
}