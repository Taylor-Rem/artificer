pub mod specialist;
pub mod worker;
pub mod background;
mod registry;
pub mod conversation;

use serde::{Deserialize, Serialize};
use anyhow::Result;
use crate::memory::Db;
use crate::Message;
use artificer_tools::registry as tool_registry;
use specialist::{ExecutionContext, ResponseMessage, Specialist};

#[derive(Debug, Clone)]
pub enum TaskType {
    /// Single execution, return immediately
    Singular,
    /// Agentic loop, handle tool calls until completion
    AgenticLoop,
}

// ============================================================================
// MACRO DEFINITION
// ============================================================================

macro_rules! define_tasks {
    (
        $(
            $variant:ident {
                title: $title:literal,
                description: $desc:literal,
                task_id: $task_id:literal,
                specialist: $specialist:expr,
                context: $context:expr,
                task_type: $task_type:expr,
                instructions: $instructions:literal,
                switches_to: [$($switch:ident),*],
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub enum Task {
            $($variant),*
        }

        impl Task {
            /// Get all task variants for iteration
            pub fn all() -> &'static [Task] {
                &[$(Task::$variant),*]
            }

            /// Parse task from string identifier
            pub fn from_str(s: &str) -> Option<Self> {
                match s {
                    $($title => Some(Task::$variant),)*
                    _ => None,
                }
            }

            /// Get the string identifier for this task
            pub fn title(&self) -> &'static str {
                match self {
                    $(Task::$variant => $title),*
                }
            }

            /// Get human-readable description
            pub fn description(&self) -> &'static str {
                match self {
                    $(Task::$variant => $desc),*
                }
            }

            /// Get database task ID
            pub fn task_id(&self) -> i64 {
                match self {
                    $(Task::$variant => $task_id),*
                }
            }

            /// Get the specialist that handles this task
            pub fn specialist(&self) -> Specialist {
                match self {
                    $(Task::$variant => $specialist),*
                }
            }

            /// Get execution context (which GPU/port to use)
            pub fn execution_context(&self) -> ExecutionContext {
                match self {
                    $(Task::$variant => $context),*
                }
            }

            /// Get task execution type
            pub fn task_type(&self) -> TaskType {
                match self {
                    $(Task::$variant => $task_type),*
                }
            }

            /// Get base instructions for this task
            pub fn instructions(&self) -> &'static str {
                match self {
                    $(Task::$variant => $instructions),*
                }
            }

            /// Get tasks this task can switch to
            pub fn available_switches(&self) -> &'static [Task] {
                match self {
                    $(Task::$variant => &[$(Task::$switch),*]),*
                }
            }
        }
    };
}

// ============================================================================
// TASK DEFINITIONS
// ============================================================================

define_tasks! {
    TitleGeneration {
        title: "title_generation",
        description: "Generate concise titles for conversations",
        task_id: 1,
        specialist: Specialist::Quick,
        context: ExecutionContext::Background,
        task_type: TaskType::Singular,
        instructions: "Generate a concise, descriptive title (3-5 words) for this conversation. \
                       Use underscores instead of spaces. Use only alphanumeric characters and underscores. \
                       Return ONLY the title with no explanation, punctuation, or quotes.",
        switches_to: [],
    },
    Summarization {
        title: "summarization",
        description: "Summarize conversations and text",
        task_id: 1,
        specialist: Specialist::Quick,
        context: ExecutionContext::Background,
        task_type: TaskType::Singular,
        instructions: "Summarize the following text concisely in 2-3 sentences. \
                       Focus on the main points and key takeaways.",
        switches_to: [],
    },
    Translation {
        title: "translation",
        description: "Translate text between languages",
        task_id: 1,
        specialist: Specialist::Quick,
        context: ExecutionContext::Background,
        task_type: TaskType::Singular,
        instructions: "Translate the following text accurately while preserving tone and meaning. \
                       Maintain the original formatting and structure.",
        switches_to: [],
    },
    Extraction {
        title: "extraction",
        description: "Extract specific information from text",
        task_id: 1,
        specialist: Specialist::Quick,
        context: ExecutionContext::Background,
        task_type: TaskType::Singular,
        instructions: "Extract and return only the requested information from the text. \
                       Be precise and concise.",
        switches_to: [],
    },
    Chat {
        title: "chat",
        description: "Interactive chat with tool calling",
        task_id: 2,
        specialist: Specialist::ToolCaller,
        context: ExecutionContext::Interactive,
        task_type: TaskType::AgenticLoop,
        instructions: "You are Artificer, a helpful AI assistant. Engage naturally with the user, \
                       provide thoughtful responses, and use available tools when appropriate. \
                       Maintain context from the conversation history.",
        switches_to: [Research, CodeReview],
    },
    CodeReview {
        title: "code_review",
        description: "Review code for issues and improvements",
        task_id: 4,
        specialist: Specialist::Coder,
        context: ExecutionContext::Interactive,
        task_type: TaskType::AgenticLoop,
        instructions: "Review the provided code for potential issues, improvements, and best practices. \
                       Provide constructive feedback with specific suggestions.",
        switches_to: [Chat],
    },
    Research {
        title: "research",
        description: "Deep research with reasoning",
        task_id: 3,
        specialist: Specialist::Reasoner,
        context: ExecutionContext::Interactive,
        task_type: TaskType::AgenticLoop,
        instructions: "Research the given topic thoroughly. Provide well-sourced information, \
                       consider multiple perspectives, and organize findings clearly.",
        switches_to: [Chat],
    },
    MemoryExtraction {
        title: "memory_extraction",
        description: "Extract learnings from conversations",
        task_id: 1,
        specialist: Specialist::Quick,
        context: ExecutionContext::Background,
        task_type: TaskType::Singular,
        instructions: "Review this conversation and extract key factual information that would be \
                       useful to remember for future sessions. Focus on:\n\
                       - User preferences and settings\n\
                       - System information (OS, paths, configurations)\n\
                       - Persistent context (project names, file locations)\n\
                       - Important decisions or constraints\n\n\
                       Return a JSON array of memories in this format:\n\
                       [{\"key\": \"operating_system\", \"value\": \"Ubuntu 22.04\"},\n\
                        {\"key\": \"home_directory\", \"value\": \"/home/tweenson\"}]\n\n\
                       Only extract facts that will remain true across sessions. Ignore ephemeral details.",
        switches_to: [],
    },
}

// ============================================================================
// TASK IMPLEMENTATION
// ============================================================================

impl Task {
    pub fn build_system_prompt(&self, db: &Db, device_id: i64) -> Result<String> {
        let base_instructions = self.instructions();

        // Get memories for this device and task
        let memories = db.query(
            "SELECT key, value, memory_type, confidence
             FROM local_task_data
             WHERE device_id = ?1
               AND task_id IN (
                   SELECT id FROM tasks WHERE title IN (?2, 'general')
               )
             ORDER BY
               CASE memory_type
                 WHEN 'fact' THEN 1
                 WHEN 'context' THEN 2
                 WHEN 'preference' THEN 3
               END,
               confidence DESC,
               updated_at DESC",
            rusqlite::params![device_id, self.title()]
        )?;

        let memories: Vec<serde_json::Value> = serde_json::from_str(&memories)?;

        if memories.is_empty() {
            return Ok(base_instructions.to_string());
        }

        // Separate by type
        let facts: Vec<_> = memories.iter()
            .filter(|m| m["memory_type"].as_str() == Some("fact"))
            .collect();

        let preferences: Vec<_> = memories.iter()
            .filter(|m| m["memory_type"].as_str() == Some("preference"))
            .collect();

        let context: Vec<_> = memories.iter()
            .filter(|m| m["memory_type"].as_str() == Some("context"))
            .collect();

        let mut prompt = base_instructions.to_string();

        // Add facts (high confidence, objective)
        if !facts.is_empty() {
            prompt.push_str("\n\n# System Information\n");
            for fact in facts {
                let key = fact["key"].as_str().unwrap_or("");
                let value = fact["value"].as_str().unwrap_or("");
                let confidence = fact["confidence"].as_f64().unwrap_or(1.0);

                // Only include high-confidence facts
                if confidence >= 0.8 {
                    prompt.push_str(&format!("- {}: {}\n", key, value));
                }
            }
        }

        // Add context (what user is currently doing)
        if !context.is_empty() {
            prompt.push_str("\n# Current Context\n");
            for ctx in context {
                let key = ctx["key"].as_str().unwrap_or("");
                let value = ctx["value"].as_str().unwrap_or("");
                prompt.push_str(&format!("- {}: {}\n", key, value));
            }
        }

        // Add preferences (how user likes things)
        if !preferences.is_empty() {
            prompt.push_str("\n# User Preferences\n");
            for pref in preferences {
                let key = pref["key"].as_str().unwrap_or("");
                let value = pref["value"].as_str().unwrap_or("");
                let confidence = pref["confidence"].as_f64().unwrap_or(0.8);

                // Phrase preferences as preferences, not rules
                if confidence >= 0.7 {
                    prompt.push_str(&format!("- User prefers: {} ({})\n", value, key));
                } else {
                    prompt.push_str(&format!("- User sometimes prefers: {} ({})\n", value, key));
                }
            }
            prompt.push_str("\nNote: These are preferences, not strict rules. \
                            Adapt based on the specific request.\n");
        }

        Ok(prompt)
    }

    /// Execute this task with the appropriate execution mode
    pub async fn execute(
        &self,
        messages: Vec<Message>,
        streaming: bool,
    ) -> Result<ResponseMessage> {
        match self.task_type() {
            TaskType::Singular => {
                let specialist = self.specialist();
                let url = self.execution_context().url();
                specialist.execute(url, self, messages, streaming).await
            }
            TaskType::AgenticLoop => {
                self.execute_agentic_loop(messages, streaming).await
            }
        }
    }

    /// Execute with system prompt automatically added
    pub async fn execute_with_prompt(
        &self,
        user_messages: Vec<Message>,
        db: &Db,
        device_id: i64,
        streaming: bool,
    ) -> Result<ResponseMessage> {
        let system_prompt = self.build_system_prompt(db, device_id)?;

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
            let response = specialist.execute(url, self, messages.clone(), streaming).await?;

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

                    // Ollama expects tool results as assistant messages, not "tool" role
                    messages.push(Message {
                        role: "assistant".to_string(),
                        content: Some(result),
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
