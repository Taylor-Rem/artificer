use serde::{Deserialize, Serialize};
use artificer_shared::{Message, Tool, ToolCall};

/// Response from the LLM (non-streaming)
#[derive(Debug, Clone, Deserialize)]
pub struct LlmResponse {
    pub message: Message,
}

/// Request to the LLM
#[derive(Debug, Clone, Serialize)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

impl LlmRequest {
    pub fn new(model: String, messages: Vec<Message>) -> Self {
        Self {
            model,
            messages,
            tools: None,
            tool_choice: None,
            stream: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Force the model to call at least one tool. Prevents free-form text responses.
    pub fn with_tool_choice(mut self, choice: serde_json::Value) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    pub fn with_streaming(mut self, stream: bool) -> Self {
        self.stream = Some(stream);
        self
    }
}

/// A single streaming chunk from the LLM
#[derive(Debug, Deserialize)]
pub struct StreamChunk {
    pub message: Option<StreamMessage>,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Deserialize)]
pub struct StreamMessage {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}
