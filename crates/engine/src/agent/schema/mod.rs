pub mod response;
pub mod context;
pub mod task;

pub use response::AgentResponse;
pub use context::AgentContext;
pub use task::Task;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Full orchestrator with task management and planning loop
    Agentic,
    /// Simple one-off LLM call, no task overhead
    OneTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRoles {
    Orchestrator,
    Specialist,
    Background
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionType {
    /// Execute tools as needed, collect raw results, return them as XML.
    /// Skips the final LLM synthesis step entirely.
    ToolProxy,

    /// Full agentic loop — reason, call tools, synthesize a conclusion.
    /// Returns an LLM-generated response.
    Agentic,
}

impl ExecutionType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "tool_proxy" => Some(Self::ToolProxy),
            "agentic" => Some(Self::Agentic),
            _ => None,
        }
    }
}

impl std::fmt::Display for ExecutionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ToolProxy => write!(f, "tool_proxy"),
            Self::Agentic => write!(f, "agentic"),
        }
    }
}
