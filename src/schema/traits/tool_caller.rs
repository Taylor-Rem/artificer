use anyhow::Result;
use serde_json::Value;

pub trait ToolCaller {
    fn use_tool(&self, tool_name: &str, args: &Value) -> Result<String>;
}
