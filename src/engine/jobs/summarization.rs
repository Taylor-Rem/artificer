// jobs/summarization.rs
use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use super::JobContext;

pub fn execute<'a>(
    ctx: &'a JobContext<'_>,
    args: &'a Value
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let conversation_id = args["conversation_id"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

        let messages_json = ctx.db.query(
            "SELECT role, message FROM message
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

        let summary = ctx.helper.summarize(&text).await?;

        ctx.db.execute(
            "UPDATE conversation SET summary = ?1 WHERE id = ?2",
            rusqlite::params![summary, conversation_id]
        )?;

        Ok(summary)
    })
}