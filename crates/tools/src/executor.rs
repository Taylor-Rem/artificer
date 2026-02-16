use anyhow::Result;
use serde_json::Value;

pub enum ToolExecutor {
    Local,
    Remote { device_id: u64 },
}

impl ToolExecutor {
    pub fn execute(&self, tool_name: &str, args: &Value) -> Result<String> {
        match self {
            ToolExecutor::Local => {
                crate::registry::use_tool(tool_name, args)
            }
            ToolExecutor::Remote { .. } => {
                // TODO: Implement remote tool execution via HTTP to envoy
                Err(anyhow::anyhow!("Remote tool execution not yet implemented"))
            }
        }
    }
}
