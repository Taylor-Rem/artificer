use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::toolbelt::{ToolBelt, ToolSchema};

// Ollama Tool format (request)
#[derive(Serialize, Clone, Debug)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunction,
}

#[derive(Serialize, Clone, Debug)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: ToolParameters,
}

#[derive(Serialize, Clone, Debug)]
pub struct ToolParameters {
    #[serde(rename = "type")]
    pub param_type: String,
    pub properties: Value,
    pub required: Vec<String>,
}

// Ollama ToolCall format (response)
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ToolCall {
    pub function: ToolCallFunction,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: Value,
}

// Convert ToolSchema to Ollama's Tool format
impl From<&ToolSchema> for Tool {
    fn from(schema: &ToolSchema) -> Self {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &schema.parameters {
            properties.insert(
                param.name.to_string(),
                json!({
                    "type": param.type_name,
                    "description": param.description
                }),
            );
            if param.required {
                required.push(param.name.to_string());
            }
        }

        Tool {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: schema.name.to_string(),
                description: schema.description.to_string(),
                parameters: ToolParameters {
                    param_type: "object".to_string(),
                    properties: Value::Object(properties),
                    required,
                },
            },
        }
    }
}

pub trait ToolCaller {
    fn toolbelts(&self) -> &[Box<dyn ToolBelt + Send + Sync>];

    fn get_tools(&self) -> Vec<Tool> {
        self.toolbelts()
            .iter()
            .flat_map(|belt| belt.get_tool_schemas())
            .map(|schema| Tool::from(&schema))
            .collect()
    }

    fn execute_tool(&self, name: &str, args: &Value) -> Result<String> {
        for belt in self.toolbelts() {
            if belt.list_tools().contains(&name) {
                return belt.use_tool(name, args);
            }
        }
        Err(anyhow!("Tool '{}' not found", name))
    }
}
