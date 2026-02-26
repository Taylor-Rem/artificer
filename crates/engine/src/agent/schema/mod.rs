pub mod response;
pub mod context;
pub mod prompts;
mod task;

pub use response::AgentResponse;
pub use context::AgentContext;
pub use task::{ Task};

pub enum AgentRoles {
    Orchestrator,
    Specialist,
    Background
}