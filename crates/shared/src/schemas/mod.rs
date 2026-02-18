// crates/shared/src/mod
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub enum ToolLocation {
    Server,
    Client,
}

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Vec<ParameterSchema>,
    pub location: ToolLocation,
}

#[derive(Debug, Clone)]
pub struct ParameterSchema {
    pub name: &'static str,
    pub type_name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

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

pub type ToolHandler = fn(&Value) -> anyhow::Result<String>;