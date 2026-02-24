pub mod db;
pub mod schemas;
pub mod executor;
pub mod events;
pub mod tools;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use rusqlite;
pub use schemas::{ParameterSchema, Tool, ToolLocation, ToolSchema};
pub use tools::{get_tools, get_tools_for, use_tool, get_tool_schema};

// Shared message types used by both engine and shared DB layer
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    pub function: FunctionCall,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Value,
}