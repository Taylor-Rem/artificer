// crates/engine/src/agent/implementations/mod.rs

pub mod orchestrator;
pub mod specialists;
pub mod background;

pub use orchestrator::Orchestrator;

// Re-export all specialist instances for easy access
pub use specialists::{
    WebResearcher,
    FileSmith,
};

// Re-export all background agents
pub use background::{
    TitleGeneration,
    Summarization,
    MemoryExtraction,
};

// Specialist registry - all specialist definitions in one place
use crate::agent::SpecialistDefinition;

pub static SPECIALISTS: &[SpecialistDefinition] = &[
    specialists::WebResearcher::DEFINITION,
    specialists::FileSmith::DEFINITION,
];

pub struct BackgroundAgentDefinition {
    pub name: &'static str,
}

pub static BACKGROUND_AGENT_DEFINITIONS: &[BackgroundAgentDefinition] = &[
    BackgroundAgentDefinition { name: "title_generation" },
    BackgroundAgentDefinition { name: "summarization" },
    BackgroundAgentDefinition { name: "memory_extraction" },
];