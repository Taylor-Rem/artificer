use crate::define_agents;
use crate::agent::{Agent, AgentContext, AgentRoles};

define_agents! {
    Orchestrator: AgentRoles::Orchestrator => {
        description: "Coordinates tasks and manages workflow",
        system_prompt: "",
        tools: Some(vec![/* orchestrator tools */]),
    },
}