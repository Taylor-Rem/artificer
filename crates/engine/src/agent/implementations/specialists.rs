use crate::define_agents;
use crate::agent::{AgentRoles, ExecutionMode};

define_agents! {
    Orchestrator: AgentRoles::Orchestrator => {
        description: "Primary orchestrator that coordinates tasks and manages workflow",
        execution_mode: ExecutionMode::Agentic,
        system_prompt: "",
        toolbelts: [],
        task_tools: true,
        delegation_tools: true,
    },

    FileSmith: AgentRoles::Specialist => {
        description: "File system specialist for reading, writing, and manipulating files",
        execution_mode: ExecutionMode::Agentic,
        system_prompt: include_str!("../prompts/file_smith.txt"),
        toolbelts: ["FileSmith::"],
        task_tools: true,
        specialist_tools: true,
    },

    WebResearcher: AgentRoles::Specialist => {
        description: "Web research specialist for searching and fetching web content",
        execution_mode: ExecutionMode::Agentic,
        system_prompt: include_str!("../prompts/web_researcher.txt"),
        toolbelts: ["WebSearch::"],
        task_tools: true,
        specialist_tools: true,
    },

    Archivist: AgentRoles::Specialist => {
        description: "Conversation history and database query specialist",
        execution_mode: ExecutionMode::Agentic,
        system_prompt: include_str!("../prompts/archivist.txt"),
        toolbelts: ["Archivist::"],
        task_tools: true,
        specialist_tools: true,
    },

    TitleGenerator: AgentRoles::Background => {
        description: "Generates concise titles for conversations",
        execution_mode: ExecutionMode::OneTime,
        system_prompt: "You generate concise, descriptive titles (3-6 words) for conversations. Output only the title, no explanation.",
        toolbelts: [],
        task_tools: false,
    },
}
