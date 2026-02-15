use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use crate::task::worker::JobContext;
use crate::task::specialist::ExecutionContext;
use crate::Message;
use crate::task::Task;

pub fn execute<'a>(
    ctx: &'a JobContext<'_>,
    args: &'a Value
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let task_history_id = args["task_history_id"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing task_history_id"))?;

        // Get task type and messages
        let task_info = ctx.db.query_row_optional(
            "SELECT t.title, et.task_id
             FROM task_historys et
             JOIN tasks t ON et.task_id = t.id
             WHERE et.id = ?1",
            rusqlite::params![task_history_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        )?.ok_or_else(|| anyhow::anyhow!("Executed task not found"))?;

        let (task_title, task_id) = task_info;

        let messages_json = ctx.db.query(
            "SELECT role, message FROM messages
             WHERE task_history_id = ?1
             ORDER BY \"order\"",
            rusqlite::params![task_history_id]
        )?;

        let messages: Vec<Value> = serde_json::from_str(&messages_json)?;
        let task_text: String = messages.iter()
            .map(|m| format!("{}: {}",
                             m["role"].as_str().unwrap_or(""),
                             m["message"].as_str().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n");

        // Ask LLM to extract learnings
        let extraction_prompt = format!(
            "Task context: {}\n\nTask:\n{}\n\nExtract facts.",
            task_title, task_text
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

        let facts_json = response.content.unwrap_or_default();
        let facts: Vec<Value> = serde_json::from_str(&facts_json)?;

        // Store learnings
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        for fact in &facts {
            let key = fact["key"].as_str().unwrap_or("");
            let value = fact["value"].as_str().unwrap_or("");

            if key.is_empty() || value.is_empty() {
                continue;
            }

            // Determine if this should be general or task-specific
            let target_task_id = if is_general_fact(key) { 1 } else { task_id };

            ctx.db.execute(
                "INSERT INTO local_task_data (task_id, task_history_id, key, value, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(task_id, key) DO UPDATE SET
                     value = excluded.value,
                     updated_at = excluded.updated_at,
                     task_history_id = excluded.task_history_id",
                rusqlite::params![target_task_id, task_history_id, key, value, now, now]
            )?;
        }

        Ok(format!("Extracted {} facts", facts.len()))
    })
}

fn is_general_fact(key: &str) -> bool {
    // Facts that apply across all tasks
    matches!(key,
        "operating_system" |
        "home_directory" |
        "user_name" |
        "preferred_language" |
        "timezone"
    )
}