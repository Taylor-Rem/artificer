use serde::{Deserialize, Serialize};
use serde_json::Value;
use anyhow::Result;
use reqwest::Client;

use crate::Message;
use crate::schema::Tool;

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

pub trait Agent: Send + Sync {
    fn ollama_url(&self) -> &'static str;
    fn model(&self) -> &'static str;
    fn client(&self) -> Client;

    async fn make_request(&self, messages: &Vec<Message>, tools: Option<Vec<Tool>>) -> Result<ResponseMessage> {
        let request = ChatRequest {
            model: self.model().to_string(),
            messages: messages.clone(),
            stream: false,
            tools,
        };
        let response = self
            .client()
            .post(self.ollama_url())
            .json(&request)
            .send()
            .await?
            .json::<ChatResponse>()
            .await?;
        Ok(response.message)
    }
}
