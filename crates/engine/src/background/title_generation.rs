use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use artificer_shared::db::{Db, sanitize_title};
use artificer_shared::rusqlite;

use crate::pool::GpuHandle;
use crate::specialist::Specialist;
use crate::Message;

pub async fn execute(
    db: &Db,
    gpu: &GpuHandle,
    client: &Client,
    args: &Value,
) -> Result<String> {
    let conversation_id = args["conversation_id"].as_i64()
        .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

    let user_message = args["user_message"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing user_message"))?;

    let specialist = Specialist::find("title_generation")
        .ok_or_else(|| anyhow::anyhow!("title_generation specialist not found"))?;

    let messages = specialist.build_messages(
        vec![Message {
            role: "user".to_string(),
            content: Some(user_message.to_string()),
            tool_calls: None,
        }],
        None,
    );

    let response = specialist.execute(gpu, messages, None, client).await?;
    let raw_title = response.content.unwrap_or_default();
    let sanitized = sanitize_title(&raw_title);

    if sanitized.is_empty() {
        return Err(anyhow::anyhow!("Generated title was empty after sanitization"));
    }

    // Get device_id for uniqueness check
    let device_id: i64 = db.query_row_optional(
        "SELECT device_id FROM conversations WHERE id = ?1",
        rusqlite::params![conversation_id],
        |row| row.get(0),
    )?.ok_or_else(|| anyhow::anyhow!("Conversation not found"))?;

    let final_title = db.set_conversation_title(conversation_id as u64, device_id, &sanitized)?;

    Ok(final_title)
}
