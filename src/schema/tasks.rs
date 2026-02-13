use serde::{Deserialize, Serialize};
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
}

impl Task {
    pub fn task_type(&self) -> TaskType {
        match self {
            Task::TitleGeneration
            | Task::Summarization
            | Task::Translation
            | Task::Extraction => TaskType::Helper,

            Task::Chat
            | Task::CodeReview
            | Task::Research => TaskType::Specialist,
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
        }
    }
}