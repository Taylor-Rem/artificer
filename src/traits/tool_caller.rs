use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Clone)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Serialize, Clone)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub trait ToolCaller {
    fn use_tool(&self, tool_name: &str, args: &Value) -> Result<String> {
        crate::registry::use_tool(tool_name, args)
    }
}
