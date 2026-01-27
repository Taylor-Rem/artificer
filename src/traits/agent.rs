use serde::{Deserialize, Serialize};
use anyhow::Result;
use reqwest::Client;

use crate::Message;
use crate::traits::toolbelt::ToolBelt;

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
}
#[derive(Deserialize)]
pub struct ChatResponse {
    pub message: Message,
}
pub trait Agent: Send + Sync {
    fn ollama_url(&self) -> &'static str;
    fn model(&self) -> &'static str;
    fn client(&self) -> Client;
    fn system_prompt(&self) -> &'static str;
    fn toolbelts(&self) -> Vec<Box<dyn ToolBelt + Send + Sync>>;

    async fn make_request(&self, messages: &Vec<Message>) -> Result<Message> {
        let request = ChatRequest {
            model: self.model().to_string(),
            messages: messages.clone(),
            stream: false,
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