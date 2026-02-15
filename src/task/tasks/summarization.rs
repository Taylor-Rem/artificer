use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use super::JobContext;
use crate::Message;
use crate::task::Task;

pub fn execute<'a>(
    ctx: &'a JobContext<'_>,
    args: &'a Value
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let conversation_id = args["conversation_id"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

        let messages_json = ctx.db.query(
            "SELECT role, message FROM messages
             WHERE conversation_id = ?1
             ORDER BY \"order\"",
            rusqlite::params![conversation_id]
        )?;

        let messages: Vec<Value> = serde_json::from_str(&messages_json)?;
        let text: String = messages.iter()
            .map(|m| format!("{}: {}",
                             m["role"].as_str().unwrap_or(""),
                             m["message"].as_str().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n");

        let llm_messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(Task::Summarization.instructions().to_string()),
                tool_calls: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(text),
                tool_calls: None,
            },
        ];

        let response = ctx.specialist.execute(llm_messages, false).await?;
        let summary = response.content.unwrap_or_default();

        ctx.db.execute(
            "UPDATE conversations SET summary = ?1 WHERE id = ?2",
            rusqlite::params![summary, conversation_id]
        )?;

        Ok(summary)
    })
}
