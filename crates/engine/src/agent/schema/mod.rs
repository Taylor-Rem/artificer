pub mod response;
pub mod context;

pub use response::AgentResponse;
pub use context::AgentContext;


pub enum AgentRoles {
    Orchestrator,
    Specialist
}