use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    TaskSwitch {
        from: String,
        to: String,
    },
    ToolCall {
        task: String,
        tool: String,
        args: serde_json::Value,
    },
    ToolResult {
        task: String,
        tool: String,
        result: String,
        truncated: bool,
    },
    StreamChunk {
        content: String,
    },
    ResponseComplete {
        content: String,
    },
    Done {
        conversation_id: u64,
    },
    Error {
        message: String,
    },
}