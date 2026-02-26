mod schema;
pub mod implementations;
pub mod macros;
pub mod execution;

use reqwest::Client;
use artificer_shared::Tool;
pub use schema::{AgentResponse, AgentContext, AgentRoles, Task};
pub use implementations::AgentType;
pub use execution::AgentExecution;

pub struct Agent {
    pub name: &'static str,
    pub description: &'static str,
    pub role: AgentRoles,
    pub system_prompt: &'static str,
    pub tools: Vec<Tool>,
    pub client: Client
}