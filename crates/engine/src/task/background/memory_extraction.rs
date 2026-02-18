use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use artificer_shared::rusqlite;
use crate::task::worker::JobContext;
use crate::task::specialist::ExecutionContext;
use crate::Message;
use crate::task::Task;

#[derive(Deserialize, Debug)]
struct ExtractedMemory {
    key: String,
    value: String,
    memory_type: String,  // "fact", "preference", or "context"
    confidence: f64,
}

#[derive(Deserialize, Debug)]
struct ExtractionResult {
    memories: Vec<ExtractedMemory>,
    keywords: Vec<String>,
}

pub fn execute<'a>(
    ctx: &'a JobContext<'_>,
    args: &'a Value
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let conversation_id = args["conversation_id"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

        // Get task types used in this conversation
        let task_info_json = ctx.db.query(
            "SELECT DISTINCT t.title, t.id
             FROM task_history th
             JOIN tasks t ON th.task_id = t.id
             WHERE th.conversation_id = ?1",
            rusqlite::params![conversation_id]
        )?;

        let task_info: Vec<Value> = serde_json::from_str(&task_info_json)?;

        // Get all messages in the conversation
        let messages_json = ctx.db.query(
            "SELECT role, message FROM messages
             WHERE conversation_id = ?1
             ORDER BY m_order",
            rusqlite::params![conversation_id]
        )?;

        let messages: Vec<Value> = serde_json::from_str(&messages_json)?;
        let conversation_text: String = messages.iter()
            .map(|m| format!("{}: {}",
                             m["role"].as_str().unwrap_or(""),
                             m["message"].as_str().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n");

        // Build context about tasks used
        let task_context = if task_info.is_empty() {
            "chat".to_string()
        } else {
            task_info.iter()
                .filter_map(|t| t["title"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        // Ask LLM to extract both memories AND keywords
        let extraction_prompt = format!(
            "Tasks used: {}\n\nConversation:\n{}\n\n\
            Extract two types of information from this conversation:\n\n\
            1. MEMORIES - Key information classified as:\n\
            - FACT: Objective, verifiable information (OS, paths, shared, project details)\n\
            - PREFERENCE: User's subjective choices (style, tone, workflow preferences)\n\
            - CONTEXT: Current/temporary situation (what they're working on now)\n\n\
            2. KEYWORDS - Important terms, topics, and concepts that characterize this conversation.\n\
            Keywords should be:\n\
            - Single words or short phrases (1-3 words)\n\
            - Lowercase\n\
            - Descriptive of the conversation's content\n\
            - Technical terms, project names, important concepts\n\
            - Examples: 'rust', 'database design', 'memory system', 'api development'\n\n\
            Return a JSON object with this structure:\n\
            {{\n  \
              \"memories\": [\n    \
                {{\"key\": \"operating_system\", \"value\": \"Ubuntu 22.04\", \"memory_type\": \"fact\", \"confidence\": 1.0}}\n  \
              ],\n  \
              \"keywords\": [\"rust\", \"database\", \"memory extraction\", \"ollama\"]\n\
            }}\n\n\
            Rules:\n\
            - Facts should have high confidence (0.9-1.0)\n\
            - Preferences should have medium confidence (0.6-0.9)\n\
            - Context should vary based on how recent/relevant (0.5-0.9)\n\
            - Only extract information that will be useful later\n\
            - Extract 3-10 keywords that best describe this conversation\n\
            - Ignore ephemeral chat content",
            task_context, conversation_text
        );

        let llm_messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(Task::MemoryExtraction.instructions().to_string()),
                tool_calls: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(extraction_prompt),
                tool_calls: None,
            },
        ];

        let response = ctx.specialist.execute(
            ExecutionContext::Background.url(),
            &Task::MemoryExtraction,
            llm_messages,
            false,
            None,
        ).await?;

        let extraction_json = response.content.unwrap_or_default();
        let extraction: ExtractionResult = serde_json::from_str(&extraction_json)?;

        // Get device_id from conversation
        let device_id: i64 = ctx.db.query_row_optional(
            "SELECT device_id FROM conversations WHERE id = ?1",
            rusqlite::params![conversation_id],
            |row| row.get(0)
        )?.ok_or_else(|| anyhow::anyhow!("Conversation not found"))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        // Store memories
        for memory in &extraction.memories {
            if memory.key.is_empty() || memory.value.is_empty() {
                continue;
            }

            // Validate memory_type
            if !matches!(memory.memory_type.as_str(), "fact" | "preference" | "context") {
                eprintln!("Warning: Invalid memory_type '{}', skipping", memory.memory_type);
                continue;
            }

            // Determine target task (general vs task-specific)
            let target_task_id = if is_general_memory(&memory.key) { 1 } else { 2 };

            ctx.db.execute(
                "INSERT INTO local_task_data
                 (device_id, task_id, conversation_id, key, value, memory_type, confidence, created_at, updated_at, last_accessed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(device_id, task_id, key) DO UPDATE SET
                     value = excluded.value,
                     memory_type = excluded.memory_type,
                     confidence = excluded.confidence,
                     updated_at = excluded.updated_at,
                     conversation_id = excluded.conversation_id",
                rusqlite::params![
                    device_id,
                    target_task_id,
                    conversation_id,
                    memory.key,
                    memory.value,
                    memory.memory_type,
                    memory.confidence,
                    now,
                    now,
                    now
                ]
            )?;
        }

        // Store keywords
        let conn = ctx.db.lock()?;

        for keyword in &extraction.keywords {
            let keyword_lower = keyword.trim().to_lowercase();
            if keyword_lower.is_empty() {
                continue;
            }

            // Insert keyword if it doesn't exist, get its ID
            conn.execute(
                "INSERT OR IGNORE INTO keywords (keyword) VALUES (?1)",
                rusqlite::params![keyword_lower]
            )?;

            let keyword_id: i64 = conn.query_row(
                "SELECT id FROM keywords WHERE keyword = ?1",
                rusqlite::params![keyword_lower],
                |row| row.get(0)
            )?;

            // Link keyword to conversation
            conn.execute(
                "INSERT OR IGNORE INTO conversation_keywords (conversation_id, keyword_id)
                 VALUES (?1, ?2)",
                rusqlite::params![conversation_id, keyword_id]
            )?;
        }

        Ok(format!(
            "Extracted {} memories ({} facts, {} preferences, {} context) and {} keywords",
            extraction.memories.len(),
            extraction.memories.iter().filter(|m| m.memory_type == "fact").count(),
            extraction.memories.iter().filter(|m| m.memory_type == "preference").count(),
            extraction.memories.iter().filter(|m| m.memory_type == "context").count(),
            extraction.keywords.len()
        ))
    })
}

fn is_general_memory(key: &str) -> bool {
    // Facts that apply across all tasks
    matches!(key,
        "operating_system" |
        "home_directory" |
        "user_name" |
        "timezone" |
        "shell" |
        "editor"
    )
}