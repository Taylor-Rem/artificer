use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
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

        // Ask LLM to extract learnings WITH TYPES
        let extraction_prompt = format!(
            "Tasks used: {}\n\nConversation:\n{}\n\n\
            Extract key information from this conversation and classify each as:\n\
            - FACT: Objective, verifiable information (OS, paths, tools, project details)\n\
            - PREFERENCE: User's subjective choices (style, tone, workflow preferences)\n\
            - CONTEXT: Current/temporary situation (what they're working on now)\n\n\
            Return a JSON array with this structure:\n\
            [{{\n  \
              \"key\": \"operating_system\",\n  \
              \"value\": \"Ubuntu 22.04\",\n  \
              \"memory_type\": \"fact\",\n  \
              \"confidence\": 1.0\n\
            }}]\n\n\
            Rules:\n\
            - Facts should have high confidence (0.9-1.0)\n\
            - Preferences should have medium confidence (0.6-0.9)\n\
            - Context should vary based on how recent/relevant (0.5-0.9)\n\
            - Only extract information that will be useful later\n\
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
            false
        ).await?;

        let memories_json = response.content.unwrap_or_default();
        let memories: Vec<ExtractedMemory> = serde_json::from_str(&memories_json)?;

        // Get device_id from conversation
        let device_id: i64 = ctx.db.query_row_optional(
            "SELECT device_id FROM conversations WHERE id = ?1",
            rusqlite::params![conversation_id],
            |row| row.get(0)
        )?.ok_or_else(|| anyhow::anyhow!("Conversation not found"))?;

        // Store learnings with types
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        for memory in &memories {
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

        Ok(format!("Extracted {} memories ({} facts, {} preferences, {} context)",
                   memories.len(),
                   memories.iter().filter(|m| m.memory_type == "fact").count(),
                   memories.iter().filter(|m| m.memory_type == "preference").count(),
                   memories.iter().filter(|m| m.memory_type == "context").count()
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