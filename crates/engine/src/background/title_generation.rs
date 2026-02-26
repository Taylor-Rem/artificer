use anyhow::Result;
use serde_json::Value;
use artificer_shared::db::{Db, sanitize_title};
use artificer_shared::rusqlite;

use crate::pool::{AgentPool, GpuHandle};

pub async fn execute(
    db: &Db,
    gpu: &GpuHandle,
    pool: &AgentPool,
    args: &Value,
) -> Result<String> {
    let conversation_id = args["conversation_id"].as_i64()
        .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

    let user_message = args["user_message"].as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing user_message"))?;

    let agent = pool
        .get("TitleGenerator")
        .ok_or_else(|| anyhow::anyhow!("TitleGenerator agent not found in pool"))?;

    let request_body = serde_json::json!({
        "model": gpu.model,
        "messages": [
            { "role": "system", "content": agent.system_prompt },
            { "role": "user",   "content": user_message },
        ],
        "stream": false,
    });

    let url = format!("{}/api/chat", gpu.url);
    let response = pool
        .client()
        .post(&url)
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "LLM call failed with status {}",
            response.status()
        ));
    }

    let body: Value = response.json().await?;
    let raw_title = body["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();

    let sanitized = sanitize_title(&raw_title);
    if sanitized.is_empty() {
        return Err(anyhow::anyhow!("Generated title was empty after sanitization"));
    }

    let device_id: i64 = db.query_row_optional(
        "SELECT device_id FROM conversations WHERE id = ?1",
        rusqlite::params![conversation_id],
        |row| row.get(0),
    )?.ok_or_else(|| anyhow::anyhow!("Conversation not found"))?;

    let final_title = db.set_conversation_title(conversation_id as u64, device_id, &sanitized)?;

    Ok(final_title)
}
