use anyhow::Result;
use reqwest::Client;

use crate::traits::Agent;
use crate::Message;

pub struct Helper;

impl Agent for Helper {
    fn ollama_url(&self) -> &'static str { "http://localhost:11434/api/chat"  /* 3070 (GPU 0) */ }
    fn model(&self) -> &'static str { "qwen2.5:32b-instruct-q5_K_M" }
    fn client(&self) -> Client { Client::new() }
    fn system_prompt(&self) -> &'static str { "" }
}

impl Helper {
    pub async fn create_title(&self, user_message: &Message) -> Result<String> {
        Ok(self.make_request(&vec![
            Message {
                role: "system".to_string(),
                content: Some(self.system_prompt().to_string()),
                tool_calls: None,
            },
            user_message.clone()
        ], None)
            .await?
            .content
            .unwrap_or_else(|| "Untitled".to_string()))
    }
}
