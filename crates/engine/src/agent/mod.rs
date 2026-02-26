mod schema;
pub mod implementations;
pub mod macros;
pub mod execution;
pub mod tool_execution;
pub mod tool_validation;
#[cfg(test)]
mod tool_execution_tests;

use artificer_shared::Tool;
pub use schema::{AgentResponse, AgentContext, AgentRoles, ExecutionMode, Task};
pub use implementations::AgentType;
pub use execution::AgentExecution;

pub struct Agent {
    pub name: &'static str,
    pub description: &'static str,
    pub role: AgentRoles,
    pub execution_mode: ExecutionMode,
    pub system_prompt: &'static str,
    pub tools: Vec<Tool>,
}
