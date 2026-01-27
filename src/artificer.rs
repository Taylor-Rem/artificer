use reqwest::Client;

use crate::traits::{Agent, ToolBelt, ToolChest};

pub struct Artificer;

impl Agent for Artificer {
    fn ollama_url(&self) -> &'static str { "http://localhost:11435/api/chat"  /* P40 (GPU 1) */ }
    fn model(&self) -> &'static str { "qwen2.5:32b-instruct-q5_K_M" }
    fn client(&self) -> Client { Client::new() }
    fn system_prompt(&self) -> &'static str {"You are a helpful AI assistant"}
    fn toolbelts(&self) -> Vec<Box<dyn ToolBelt + Send + Sync>> {
        inventory::iter::<ToolChest>
            .into_iter()
            .map(|chest| chest.0())
            .collect()
    }
}

impl Artificer {
    pub fn new() -> Artificer {
        Artificer {}
    }
}