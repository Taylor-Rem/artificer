use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

use crate::schema::{Agent, ToolCaller};

pub struct Artificer;

impl Agent for Artificer {
    fn ollama_url(&self) -> &'static str { "http://localhost:11435/api/chat"  /* P40 (GPU 1) */ }
    fn model(&self) -> &'static str {
        // "qwen2.5:32b-instruct-q5_K_M"
        "qwen3:32b"
    }
    fn client(&self) -> Client { Client::new() }
}

impl ToolCaller for Artificer {
    fn use_tool(&self, tool_name: &str, args: &Value) -> Result<String> {
        crate::engine::registry::use_tool(tool_name, args)
    }
}
