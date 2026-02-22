pub mod specialist;
pub mod worker;
pub mod background;
pub mod conversation;

use serde::{Deserialize, Serialize};
use anyhow::Result;
use artificer_shared::{db::Db, rusqlite, tools as tool_registry};
use crate::Message;
use specialist::{ExecutionContext, ResponseMessage, Specialist};
use crate::events::EventSender;

#[derive(Debug, Clone)]
pub enum TaskType {
    /// Single execution, return immediately
    Singular,
    /// Agentic loop, handle tool calls until completion
    AgenticLoop,
}

#[derive(Deserialize)]
pub struct PipelineStep {
    pub task: String,
    pub directions: String,
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
    Router {
        title: "router",
        description: "Routes user requests to the appropriate task pipeline",
        task_id: 5,
        specialist: Specialist::Reasoner,
        context: ExecutionContext::Interactive,
        task_type: TaskType::Singular,
        instructions: "You are a request router for Artificer, a local AI assistant. \
                       Your only job is to analyze the user's message and decide how to handle it.\
                       \n\nYou have one tool: plan_tasks.\
                       \n\nALWAYS call plan_tasks. Never respond with plain text.\
                       \n\nROUTING RULES:\
                       \n- Casual conversation, greetings, simple factual questions → [{task: 'chat', directions: '<original message>'}]\
                       \n- Anything requiring web search, news, current events, research → [{task: 'web_research', directions: '<specific search instructions>'}]\
                       \n- Research that needs summarizing → [{task: 'web_research', directions: '...'}, {task: 'summarizer', directions: 'Summarize the above into a clear response'}]\
                       \n- File operations → [{task: 'file_smith', directions: '<specific file instructions>'}]\
                       \n\nDIRECTIONS:\
                       \n- Write directions as specific instructions for each specialist, not a copy of the user message.\
                       \n- For web_research, be specific: 'Search for top news headlines today and fetch 2-3 articles for full content'\
                       \n- For summarizer, specify the desired format and length.\
                       \n- Keep directions concise but complete.",
        switches_to: [],
    },
    Chat {
        title: "chat",
        description: "Conversational chat with memory access",
        task_id: 2,
        specialist: Specialist::ToolCaller,
        context: ExecutionContext::Interactive,
        task_type: TaskType::AgenticLoop,
        instructions: "You are Artificer, a local AI assistant. You handle casual conversation \
                       and simple factual questions.\
                       \n\nYou have access to the Archivist tool to look up past conversations \
                       and user preferences when relevant.\
                       \n\nBEHAVIOR:\
                       \n- Be concise and direct. Match the length of your response to the complexity of the request.\
                       \n- A greeting gets a greeting. A simple question gets a short answer.\
                       \n- Use Archivist only when the user references past conversations or you need context.\
                       \n- Do not attempt web searches or file operations — those are handled by specialists.\
                       \n- If the user asks for something outside your scope, let them know it will be handled.",
        switches_to: [],
    },
    WebResearcher {
        title: "web_research",
        description: "Web research using Brave Search",
        task_id: 6,
        specialist: Specialist::ToolCaller,
        context: ExecutionContext::Interactive,
        task_type: TaskType::AgenticLoop,
        instructions: "You are a web research specialist. You have access to Brave Search tools.\
                       \n\nBEHAVIOR:\
                       \n- Always complete the full research cycle. Search → fetch relevant articles → synthesize.\
                       \n- Never return raw search results to the user. Always read the content and summarize.\
                       \n- For news requests, use search_news first, then fetch 2-3 of the most relevant articles.\
                       \n- For general research, use search first, then fetch the most authoritative sources.\
                       \n- If the first search is not sufficient, try different search terms before giving up.\
                       \n- Return a well-organized response with sources cited at the end.\
                       \n- If content cannot be fetched, note it and move on to the next source.",
        switches_to: [],
    },
    FileSmith {
    title: "file_smith",
    description: "File and directory operations on the client device",
    task_id: 8,
    specialist: Specialist::ToolCaller,
    context: ExecutionContext::Interactive,
    task_type: TaskType::AgenticLoop,
    instructions: "You are a file system specialist. You have access to FileSmith tools \
                   that execute on the user's local device.\
                   \n\nBEHAVIOR:\
                   \n- Confirm before destructive operations (delete, overwrite) unless explicitly told not to.\
                   \n- Use file_exists before reading or modifying to avoid errors.\
                   \n- When writing code or structured content, prefer replace_text or insert_at_line over full rewrites.\
                   \n- Always report what you did with paths and results.",
    switches_to: [],
},
    Summarizer {
        title: "summarizer",
        description: "Synthesize and summarize content into clear responses",
        task_id: 7,
        specialist: Specialist::Reasoner,
        context: ExecutionContext::Interactive,
        task_type: TaskType::Singular,
        instructions: "You are a summarization specialist. You receive content from previous pipeline \
                       steps and synthesize it into a clear, well-organized response.\
                       \n\nBEHAVIOR:\
                       \n- Follow the directions provided about format and length.\
                       \n- Preserve important details while cutting noise.\
                       \n- Organize information logically with clear structure.\
                       \n- Cite sources when they are provided in the context.\
                       \n- Never add information not present in the provided content.",
        switches_to: [],
    },
}
// ============================================================================
// TASK IMPLEMENTATION
// ============================================================================

impl Task {
    pub fn build_system_prompt(&self, db: &Db, device_id: i64) -> Result<String> {
        let base_instructions = self.instructions();

        let tool_schemas = tool_registry::get_tool_schemas_for(match self {
            Task::Router => &["Router"],
            Task::Chat => &["Archivist"],
            Task::WebResearcher => &["WebSearch"],
            Task::FileSmith => &["FileSmith"],
            _ => &[],
        });

        let mut prompt = base_instructions.to_string();

        if !tool_schemas.is_empty() {
            prompt.push_str("\n\n# Available Tools\n");
            for schema in &tool_schemas {
                prompt.push_str(&format!("\n## {}\n", schema.name));
                prompt.push_str(&format!("{}\n", schema.description));
                if !schema.parameters.is_empty() {
                    prompt.push_str("Parameters:\n");
                    for param in &schema.parameters {
                        let required = if param.required { "required" } else { "optional" };
                        prompt.push_str(&format!(
                            "- `{}` ({}{}): {}\n",
                            param.name, param.type_name,
                            if param.required { ", required" } else { ", optional" },
                            param.description
                        ));
                    }
                }
            }
        }

        // Get memories for this device and task
        let memories = db.query(
            "SELECT key, value, memory_type, confidence
             FROM local_data
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
        device_id: i64,
        device_key: String,
        streaming: bool,
        events: Option<EventSender>,
    ) -> Result<ResponseMessage> {
        match self.task_type() {
            TaskType::Singular => {
                let specialist = self.specialist();
                let url = self.execution_context().url();
                specialist.execute(url, self, messages, streaming, events).await
            }
            TaskType::AgenticLoop => {
                self.execute_agentic_loop(messages, device_id, device_key, streaming, events).await
            }
        }
    }

    /// Execute with system prompt automatically added
    pub async fn execute_with_prompt(
        &self,
        user_messages: Vec<Message>,
        db: &Db,
        device_id: i64,
        device_key: String,
        streaming: bool,
        events: Option<EventSender>,
    ) -> Result<ResponseMessage> {
        let system_prompt = self.build_system_prompt(db, device_id)?;

        let mut messages = vec![Message {
            role: "system".to_string(),
            content: Some(system_prompt),
            tool_calls: None,
        }];
        messages.extend(user_messages);

        self.execute(messages, device_id, device_key, streaming, events).await
    }

    /// Agentic loop execution: keeps running until no more tool calls
    async fn execute_agentic_loop(
        &self,
        mut messages: Vec<Message>,
        device_id: i64,
        device_key: String,
        streaming: bool,
        events: Option<EventSender>,
    ) -> Result<ResponseMessage> {
        let specialist = self.specialist();
        let url = self.execution_context().url();

        loop {
            let response = specialist.execute(url, self, messages.clone(), streaming, events.clone()).await?;

            // Add assistant response to history
            messages.push(response.to_message());

            // Check if there are tool calls to process
            if let Some(tool_calls) = &response.tool_calls {
                for tool_call in tool_calls {
                    let tool_name = &tool_call.function.name;
                    let args = &tool_call.function.arguments;

                    if let Some(ref ev) = events {
                        ev.tool_call(self.title(), tool_name, args.clone());
                    } else {
                        println!("[Calling tool: {} with args: {}]", tool_name, args);
                    }
                    if tool_name == "switch_task" {
                        let target_task_name = args["task"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing task name"))?;

                        let target_task = Task::from_str(target_task_name)
                            .ok_or_else(|| anyhow::anyhow!("Unknown task: {}", target_task_name))?;

                        if let Some(ref ev) = events {
                            ev.task_switch(self.title(), target_task.title());
                        } else {
                            println!("\n[Switching from {} to {}]\n", self.title(), target_task.title());
                        }

                        messages.push(Message {
                            role: "system".to_string(),
                            content: Some(format!(
                                "Task switch: {} → {}. Continue with the current objective.",
                                self.title(),
                                target_task.title()
                            )),
                            tool_calls: None,
                        });

                        return Box::pin(target_task.execute(messages, device_id, device_key, streaming, events)).await;
                    }

                    // Determine execution strategy based on tool location
                    use artificer_shared::executor::ToolExecutor;
                    use artificer_shared::ToolLocation;

                    let result = match artificer_shared::tools::get_tool_schema(tool_name) {
                        Ok(schema) => {
                            let executor = match schema.location {
                                ToolLocation::Server => ToolExecutor::local(),
                                ToolLocation::Client => {
                                    ToolExecutor::remote(
                                        "http://localhost:8081".to_string(),
                                        device_id,
                                        device_key.clone(),
                                    )
                                }
                            };

                            executor.execute(tool_name, args).await
                                .unwrap_or_else(|e| format!("Error: {}", e))
                        }
                        Err(_) => {
                            // Fallback to local execution if schema not found
                            tool_registry::use_tool(tool_name, args)
                                .unwrap_or_else(|e| format!("Error: {}", e))
                        }
                    };

                    if let Some(ref ev) = events {
                        ev.tool_result(self.title(), tool_name, result.clone());
                    } else {
                        println!("[Tool result: {}]", result);
                    }

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
    pub async fn execute_pipeline(
        steps: Vec<PipelineStep>,
        db: &Db,
        device_id: i64,
        device_key: String,
        events: Option<EventSender>,
    ) -> Result<ResponseMessage> {
        let mut context = String::new();

        for step in steps {
            let task = Task::from_str(&step.task)
                .ok_or_else(|| anyhow::anyhow!("Unknown task in pipeline: {}", step.task))?;

            if let Some(ref ev) = events {
                ev.task_switch("pipeline", task.title());
            }

            let user_content = if context.is_empty() {
                step.directions.clone()
            } else {
                format!("{}\n\n# Context from previous step:\n{}", step.directions, context)
            };

            let response = task.execute_with_prompt(
                vec![Message {
                    role: "user".to_string(),
                    content: Some(user_content),
                    tool_calls: None,
                }],
                db,
                device_id,
                device_key.clone(),
                events.is_some(),
                events.clone(),
            ).await?;

            context = response.content.clone().unwrap_or_default();
        }

        Ok(ResponseMessage {
            role: "assistant".to_string(),
            content: Some(context),
            tool_calls: None,
        })
    }
}
