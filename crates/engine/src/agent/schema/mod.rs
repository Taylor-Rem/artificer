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
