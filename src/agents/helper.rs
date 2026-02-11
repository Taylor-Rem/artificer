use anyhow::Result;
use reqwest::Client;

use crate::traits::Agent;
use crate::Message;

pub struct Helper;

pub enum HelperTask {
    TitleGeneration,
    Summarization,
    Translation,
    Extraction,
}

impl Agent for Helper {
    fn ollama_url(&self) -> &'static str { "http://localhost:11434/api/chat" /* 3070 */ }
    fn model(&self) -> &'static str { "qwen2.5:32b-instruct-q5_K_M" }
    fn client(&self) -> Client { Client::new() }
    fn system_prompt(&self) -> &'static str { "" }
}

impl Helper {
    fn get_system_prompt(&self, task: HelperTask) -> &'static str {
        match task {
            HelperTask::TitleGeneration =>
                "Generate a concise, descriptive title (3-5 words) for this conversation. \
                 Use underscores instead of spaces. Use only alphanumeric characters and underscores. \
                 Return ONLY the title with no explanation, punctuation, or quotes.",
            HelperTask::Summarization =>
                "Summarize the following text concisely in 2-3 sentences. \
                 Focus on the main points and key takeaways.",
            HelperTask::Translation =>
                "Translate the following text accurately while preserving tone and meaning. \
                 Maintain the original formatting and structure.",
            HelperTask::Extraction =>
                "Extract and return only the requested information from the text. \
                 Be precise and concise.",
        }
    }

    pub async fn create_title(&self, user_message: &Message) -> Result<String> {
        Ok(self.make_request(&vec![
            Message {
                role: "system".to_string(),
                content: Some(self.get_system_prompt(HelperTask::TitleGeneration).to_string()),
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
                content: Some(self.get_system_prompt(HelperTask::Summarization).to_string()),
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
