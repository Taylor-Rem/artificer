use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};

use super::ToolSchema;

#[derive(Serialize, Clone, Debug)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Serialize, Clone, Debug)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSchema {
    /// Convert to Ollama/OpenAI tool format
    pub fn to_tool(&self) -> Tool {
        let mut properties = json!({});
        let mut required = vec![];

        for param in &self.parameters {
            properties[param.name] = json!({
                "type": param.type_name,
                "description": param.description
            });
            if param.required {
                required.push(param.name);
            }
        }

        Tool {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: self.name.to_string(),
                description: self.description.to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": properties,
                    "required": required
                }),
            },
        }
    }
}

pub trait ToolCaller {
    fn use_tool(&self, tool_name: &str, args: &Value) -> Result<String> {
        crate::registry::use_tool(tool_name, args)
    }
}
