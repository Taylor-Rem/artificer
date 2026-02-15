// src/task/mod.rs
pub mod specialist;
pub mod worker;
pub mod interactive;
pub mod background;
mod registry;

use serde::{Deserialize, Serialize};
use anyhow::Result;
use serde_json::{json, Value};
use crate::memory::Db;
use crate::Message;
use crate::tools::registry as tool_registry;
use specialist::{Specialist, ExecutionContext, ResponseMessage};

#[derive(Debug, Clone)]
pub enum TaskType {
    /// Single execution, return immediately
    Singular,
    /// Agentic loop, handle tool calls until completion
    AgenticLoop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Task {
    TitleGeneration,
    Summarization,
    Translation,
    Extraction,
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

    pub fn task_id(&self) -> i64 {
        // Map to database tasks table
        // 1 = general, 2 = chat, 3 = research, 4 = code_review
        match self {
            Task::Chat => 2,
            Task::Research => 3,
            Task::CodeReview => 4,
            // Background jobs use general
            Task::TitleGeneration
            | Task::Summarization
            | Task::Translation
            | Task::Extraction
            | Task::MemoryExtraction => 1,
        }
    }

    pub fn specialist(&self) -> Specialist {
        match self {
            Task::TitleGeneration => Specialist::Quick,
            Task::Summarization => Specialist::Quick,
            Task::Translation => Specialist::Quick,
            Task::Extraction => Specialist::Quick,
            Task::Chat => Specialist::ToolCaller,
            Task::CodeReview => Specialist::Coder,
            Task::Research => Specialist::Reasoner,
            Task::MemoryExtraction => Specialist::Quick,
        }
    }

    pub fn execution_context(&self) -> ExecutionContext {
        match self {
            // Interactive tasks run on P40
            Task::Chat | Task::Research | Task::CodeReview => ExecutionContext::Interactive,
            // Background jobs run on 3070
            Task::TitleGeneration
            | Task::Summarization
            | Task::Translation
            | Task::Extraction
            | Task::MemoryExtraction => ExecutionContext::Background,
        }
    }

    pub fn task_type(&self) -> TaskType {
        match self {
            Task::Chat | Task::Research => TaskType::AgenticLoop,
            _ => TaskType::Singular,
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

        // Get task-specific AND general learnings
        let memories = db.query(
            "SELECT key, value FROM local_task_data ltd 
             JOIN tasks t ON ltd.task_id = t.id 
             WHERE t.title IN (?1, 'general')
             ORDER BY ltd.updated_at DESC",
            rusqlite::params![self.title()]
        )?;

        let memories: Vec<Value> = serde_json::from_str(&memories)?;

        if memories.is_empty() {
            return Ok(base_instructions.to_string());
        }

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

    /// Execute this task with the appropriate execution mode
    pub async fn execute(
        &self,
        messages: Vec<Message>,
        streaming: bool,
    ) -> Result<ResponseMessage> {
        match self.task_type() {
            TaskType::Singular => {
                // Simple one-shot execution
                let specialist = self.specialist();
                let url = self.execution_context().url();
                specialist.execute(url, messages, streaming).await
            }
            TaskType::AgenticLoop => {
                // Agentic loop: handle tool calls until completion
                self.execute_agentic_loop(messages, streaming).await
            }
        }
    }

    /// Execute with system prompt automatically added
    pub async fn execute_with_prompt(
        &self,
        user_messages: Vec<Message>,
        db: &Db,
        streaming: bool,
    ) -> Result<ResponseMessage> {
        let system_prompt = self.build_system_prompt(db)?;

        let mut messages = vec![Message {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
        }];
        messages.extend(user_messages);

        self.execute(messages, streaming).await
    }

    /// Agentic loop execution: keeps running until no more tool calls
    async fn execute_agentic_loop(
        &self,
        mut messages: Vec<Message>,
        streaming: bool,
    ) -> Result<ResponseMessage> {
        let specialist = self.specialist();
        let url = self.execution_context().url();

        loop {
            let response = specialist.execute(url, messages.clone(), streaming).await?;

            // Add assistant response to history
            messages.push(response.to_message());

            // Check if there are tool calls to process
            if let Some(tool_calls) = &response.tool_calls {
                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    println!("[Calling tool: {} with args: {}]", tool_name, args);

                    // Check if this is a task switch
                    if tool_name == "switch_task" {
                        // Handle task switching - for now just log it
                        println!("[Task switch requested - not yet implemented]");
                        continue;
                    }

                    let result = tool_registry::use_tool(tool_name, args)
                        .unwrap_or_else(|e| format!("Error: {}", e));

                    println!("[Tool result: {}]", result);

                    // Add tool result to messages
                    messages.push(Message {
                        role: "tool".to_string(),
                        content: Some(json!({
                            "name": tool_name,
                            "result": result
                        }).to_string()),
                        tool_calls: None,
                    });
                }
                // Continue loop to process tool results
            } else {
                // No tool calls - we're done
                return Ok(response);
            }
        }
    }
}