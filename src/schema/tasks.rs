use serde::{Deserialize, Serialize};
use anyhow::Result;
use serde_json::Value;
use crate::engine::db::Db;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    Helper,
    Specialist,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Task {
    // Helper tasks
    TitleGeneration,
    Summarization,
    Translation,
    Extraction,

    // Specialist tasks
    Chat,
    CodeReview,
    Research,
    MemoryExtraction
}

impl Task {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "title_generation" => Some(Task::TitleGeneration),
            "summarization" => Some(Task::Summarization),
            "translation" => Some(Task::Translation),
            "extraction" => Some(Task::Extraction),
            "chat" => Some(Task::Chat),
            "code_review" => Some(Task::CodeReview),
            "research" => Some(Task::Research),
            "memory_extraction" => Some(Task::MemoryExtraction),
            _ => None,
        }
    }

    pub fn task_type(&self) -> TaskType {
        match self {
            Task::TitleGeneration
            | Task::Summarization
            | Task::Translation
            | Task::Extraction => TaskType::Helper,

            Task::Chat
            | Task::CodeReview
            | Task::Research
            | Task::MemoryExtraction => TaskType::Specialist
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Task::TitleGeneration => "title_generation",
            Task::Summarization => "summarization",
            Task::Translation => "translation",
            Task::Extraction => "extraction",
            Task::Chat => "chat",
            Task::CodeReview => "code_review",
            Task::Research => "research",
            Task::MemoryExtraction => "memory_extraction"
        }
    }

    pub fn instructions(&self) -> &'static str {
        match self {
            Task::TitleGeneration =>
                "Generate a concise, descriptive title (3-5 words) for this conversation. \
                 Use underscores instead of spaces. Use only alphanumeric characters and underscores. \
                 Return ONLY the title with no explanation, punctuation, or quotes.",

            Task::Summarization =>
                "Summarize the following text concisely in 2-3 sentences. \
                 Focus on the main points and key takeaways.",

            Task::Translation =>
                "Translate the following text accurately while preserving tone and meaning. \
                 Maintain the original formatting and structure.",

            Task::Extraction =>
                "Extract and return only the requested information from the text. \
                 Be precise and concise.",

            Task::Chat =>
                "You are Artificer, a helpful AI assistant. Engage naturally with the user, \
                 provide thoughtful responses, and use available tools when appropriate. \
                 Maintain context from the conversation history.",

            Task::CodeReview =>
                "Review the provided code for potential issues, improvements, and best practices. \
                 Provide constructive feedback with specific suggestions.",

            Task::Research =>
                "Research the given topic thoroughly. Provide well-sourced information, \
                 consider multiple perspectives, and organize findings clearly.",
            
            Task::MemoryExtraction =>
                "Review this conversation and extract key factual information that would be \
                 useful to remember for future sessions. Focus on:\n\
                 - User preferences and settings\n\
                 - System information (OS, paths, configurations)\n\
                 - Persistent context (project names, file locations)\n\
                 - Important decisions or constraints\n\n\
                 Return a JSON array of memories in this format:\n\
                 [{\"key\": \"operating_system\", \"value\": \"Ubuntu 22.04\"},\n\
                  {\"key\": \"home_directory\", \"value\": \"/home/tweenson\"}]\n\n\
                 Only extract facts that will remain true across sessions. Ignore ephemeral details.",
        }
    }
    pub fn build_system_prompt(&self, db: &Db) -> Result<String> {
        let base_instructions = self.instructions();

        // Get task-specific memory
        let memories = db.query(
            "SELECT key, value FROM task_memory WHERE task_name = ?1",
            rusqlite::params![self.title()]
        )?;

        let memories: Vec<Value> = serde_json::from_str(&memories)?;

        if memories.is_empty() {
            return Ok(base_instructions.to_string());
        }

        // Build memory section
        let memory_section = memories.iter()
            .map(|m| format!("- {}: {}",
                             m["key"].as_str().unwrap_or(""),
                             m["value"].as_str().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(format!(
            "{}\n\n# Remembered Context\n{}\n",
            base_instructions,
            memory_section
        ))
    }
}
