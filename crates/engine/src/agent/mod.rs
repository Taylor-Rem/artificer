mod schema;
pub mod implementations;
pub mod macros;
pub mod execution;
pub mod tool_execution;
pub mod tool_validation;
mod llm_types;
mod llm_client;
mod mode_detection;
mod delegation_tools;

#[cfg(test)]
mod tool_execution_tests;

use artificer_shared::Tool;
pub use schema::{AgentResponse, AgentContext, AgentRoles, ExecutionMode, Task};
pub use implementations::AgentType;
pub use execution::AgentExecution;
pub use tool_execution::ToolExecutionContext;
pub use mode_detection::{detect_specialist_mode, SpecialistMode};

#[derive(Debug, Clone)]
pub struct Agent {
    pub name: &'static str,
    pub description: &'static str,
    pub role: AgentRoles,
    pub execution_mode: ExecutionMode,
    pub system_prompt: &'static str,
    pub tools: Vec<Tool>,
}
