use anyhow::Result;
use artificer_shared::{Message, Tool};
use crate::agent::{Agent, AgentRoles};

pub struct Orchestrator {}

impl Agent for Orchestrator {
    fn name(&self) -> &'static str { "Orchestrator" }
    fn system_prompt(&self) -> String { "".to_string() }
    fn role(&self) -> AgentRoles { AgentRoles::Orchestrator }
    fn requires_full_context(&self) -> bool { true }
    fn tools(&self) -> Option<Vec<Tool>> { None }
}

// impl Orchestrator {
// }