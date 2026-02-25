use crate::define_agents;
use crate::agent::{Agent, AgentContext, AgentRoles};
use artificer_shared::Tool;

// Import task tools const
use crate::orchestrator::tools::TASK_TOOLS;

define_agents! {
    Orchestrator: AgentRoles::Orchestrator => {
        description: "Coordinates tasks and manages workflow",
        system_prompt: "You are an orchestrator...",
        tools: Some(vec![/* orchestrator-specific tools */]),
        task_tools: true,  // Gets task management tools
    },

    WebResearcher: AgentRoles::Specialist => {
        description: "Searches the web for information",
        system_prompt: "You are a web research specialist...",
        tools: Some(vec![/* web search tools */]),
        task_tools: true,  // Also gets task tools
    },

    FileSmith: AgentRoles::Specialist => {
        description: "Manages files and documents",
        system_prompt: "You are a file system specialist...",
        tools: Some(vec![/* file tools */]),
        task_tools: true,
    },
}