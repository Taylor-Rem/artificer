pub mod engine;
pub mod schema;
pub mod services;
pub mod agents;
pub mod toolbelts;
use serde::{Deserialize, Serialize};
use schema::ToolCall;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}
