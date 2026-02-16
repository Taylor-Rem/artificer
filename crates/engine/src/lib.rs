pub mod api;
pub mod memory;
pub mod services;
pub mod state;
pub mod task;

use serde::{Deserialize, Serialize};
use task::specialist::ToolCall;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}
