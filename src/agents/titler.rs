use reqwest::Client;

use crate::traits::Agent;

pub struct Titler;

impl Agent for Titler {
    fn ollama_url(&self) -> &'static str { "http://localhost:11434/api/chat"  /* 3070 (GPU 0) */ }
    fn model(&self) -> &'static str { "qwen2.5:32b-instruct-q5_K_M" }
    fn client(&self) -> Client { Client::new() }
    fn system_prompt(&self) -> &'static str { "A user has started a conversation. Please review the user's request and return a short snake case title for the conversation" }
}