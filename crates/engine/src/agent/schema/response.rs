pub struct AgentResponse {
    pub content: String,
    pub success: bool
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