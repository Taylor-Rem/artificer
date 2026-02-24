/// Response from an agent execution
#[derive(Debug, Clone)]
pub struct AgentResponse {
    /// The final content/result from the agent
    pub content: String,

    /// Whether the goal was successfully completed
    pub success: bool,
}

impl AgentResponse {
    pub fn complete(content: String) -> Self {
        Self {
            content,
            success: true,
        }
    }

    pub fn failed(content: String) -> Self {
        Self {
            content,
            success: false,
        }
    }
}