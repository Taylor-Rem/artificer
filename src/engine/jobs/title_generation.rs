// jobs/title_generation.rs
use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use super::JobContext;
use crate::Message;
use crate::services::title::sanitize_title;

pub fn execute<'a>(
    ctx: &'a JobContext<'_>,
    args: &'a Value
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let conversation_id = args["conversation_id"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

        let user_message_val = args.get("message")
            .ok_or_else(|| anyhow::anyhow!("Missing message in arguments"))?;

        let message = Message {
            role: user_message_val["role"].as_str().unwrap_or("user").to_string(),
            content: user_message_val["content"].as_str().map(String::from),
            tool_calls: None,
        };

        let raw_title = ctx.helper.create_title(&message).await?;
        let sanitized = sanitize_title(&raw_title);

        if sanitized.is_empty() {
            return Err(anyhow::anyhow!("Generated title was empty after sanitization"));
        }

        let final_title = if ctx.title_service.title_exists(&sanitized) {
            ctx.title_service.find_available_title(&sanitized)
        } else {
            sanitized
        };

        ctx.db.execute(
            "UPDATE conversation SET title = ?1 WHERE id = ?2",
            rusqlite::params![final_title, conversation_id]
        )?;

        Ok(final_title)
    })
}