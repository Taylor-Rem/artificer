use anyhow::Result;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use crate::task::worker::JobContext;
use crate::task::specialist::ExecutionContext;
use crate::Message;
use crate::services::title::sanitize_title;
use crate::task::Task;

pub fn execute<'a>(
    ctx: &'a JobContext<'_>,
    args: &'a Value
) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let th_id = args["th_id"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing task_history_id"))?;

        let user_message_val = args.get("message")
            .ok_or_else(|| anyhow::anyhow!("Missing message in arguments"))?;

        let message = Message {
            role: user_message_val["role"].as_str().unwrap_or("user").to_string(),
            content: user_message_val["content"].as_str().map(String::from),
            tool_calls: None,
        };

        let messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(Task::TitleGeneration.instructions().to_string()),
                tool_calls: None,
            },
            message,
        ];

        let response = ctx.specialist.execute(ExecutionContext::Background.url(), &Task::TitleGeneration, messages, false).await?;
        let raw_title = response.content.unwrap_or_default();
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
            "UPDATE task_history SET title = ?1 WHERE id = ?2",
            rusqlite::params![final_title, th_id]
        )?;

        Ok(final_title)
    })
}
