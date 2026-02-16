// src/task/specialist.rs
use serde::{Deserialize, Serialize};
use serde_json::Value;
use anyhow::Result;
use reqwest::Client;
use futures_util::StreamExt;
use std::io::{self, Write};

use crate::Message;
use artificer_tools::Tool;
use crate::task::Task;

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

#[derive(Debug, Clone)]
pub enum ExecutionContext {
    Interactive,
    Background,
}

impl ExecutionContext {
    pub fn url(&self) -> &'static str {
        match self {
            ExecutionContext::Interactive => "http://localhost:11435/api/chat",
            ExecutionContext::Background => "http://localhost:11434/api/chat",
        }
    }
}

pub enum Specialist {
    ToolCaller,
    Reasoner,
    Quick,
    Coder,
}

impl Specialist {
    pub fn model(&self) -> &'static str {
        match self {
            Specialist::Quick => "qwen3:8b",
            Specialist::ToolCaller | Specialist::Reasoner | Specialist::Coder => "qwen3:32b",
        }
    }

    pub fn tools(&self, current_task: &Task) -> Vec<Tool> {
        use artificer_tools::registry as tool_registry;
        use crate::task::registry as task_registry;

        let mut tools = match self {
            Specialist::ToolCaller => tool_registry::get_tools(),
            Specialist::Coder => tool_registry::get_tools_for(&["FileSmith"]),
            Specialist::Reasoner | Specialist::Quick => vec![],
        };

        // Add task switching capability if this is an interactive task
        if matches!(current_task.execution_context(), ExecutionContext::Interactive) {
            tools.extend(task_registry::get_available_tasks(current_task));
        }

        tools
    }

    pub async fn execute(
        &self,
        url: &str,
        current_task: &Task,
        messages: Vec<Message>,
        streaming: bool,
    ) -> Result<ResponseMessage> {
        let tools = self.tools(current_task);
        let tools_option = if tools.is_empty() { None } else { Some(tools) };

        if streaming {
            self.execute_streaming(url, messages, tools_option).await
        } else {
            self.execute_standard(url, messages, tools_option).await
        }
    }

    async fn execute_standard(
        &self,
        url: &str,
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
            .post(url)
            .json(&request)
            .send()
            .await?
            .json::<ChatResponse>()
            .await?;

        Ok(response.message)
    }

    async fn execute_streaming(
        &self,
        url: &str,
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
            .post(url)
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

                if chunk.message.tool_calls.is_some() {
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
                if chunk.message.tool_calls.is_some() {
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
