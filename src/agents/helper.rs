use anyhow::Result;
use reqwest::Client;

use crate::schema::Agent;
use crate::schema::Task;
use crate::Message;

pub struct Helper;

impl Agent for Helper {
    fn ollama_url(&self) -> &'static str { "http://localhost:11434/api/chat" /* 3070 */ }
    fn model(&self) -> &'static str { "qwen3:8b" }
    fn client(&self) -> Client { Client::new() }
}

impl Helper {
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
