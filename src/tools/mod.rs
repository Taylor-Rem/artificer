pub mod toolbelts;
pub mod registry;

use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};

pub trait ToolCaller {
    fn use_tool(&self, tool_name: &str, args: &Value) -> Result<String>;
}

/// Schema definition for a tool (used internally)
#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Vec<ParameterSchema>,
}

/// Schema for a single parameter
#[derive(Debug, Clone)]
pub struct ParameterSchema {
    pub name: &'static str,
    pub type_name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

/// Ollama/OpenAI tool format (for API requests)
#[derive(Serialize, Clone, Debug)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function definition in Ollama/OpenAI format
#[derive(Serialize, Clone, Debug)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Type alias for tool handler functions used in the registry
pub type ToolHandler = fn(&Value) -> Result<String>;

impl ToolSchema {
    /// Convert internal schema to Ollama/OpenAI tool format
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
