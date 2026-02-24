use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use artificer_shared::rusqlite;
use artificer_shared::db::Db;

use crate::pool::GpuHandle;
use crate::specialist::Specialist;
use crate::Message;

pub async fn execute(
    db: &Db,
    gpu: &GpuHandle,
    client: &Client,
    args: &Value,
    context_messages: Option<&[Value]>,
) -> Result<String> {
    let conversation_id = args["conversation_id"].as_i64()
        .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

    let messages = context_messages
        .ok_or_else(|| anyhow::anyhow!("Summarization requires conversation context"))?;

    let text: String = messages.iter()
        .map(|m| format!("{}: {}",
            m["role"].as_str().unwrap_or(""),
            m["message"].as_str().unwrap_or("")))
        .collect::<Vec<_>>()
        .join("\n");

    let specialist = Specialist::find("summarization")
        .ok_or_else(|| anyhow::anyhow!("summarization specialist not found"))?;

    let llm_messages = specialist.build_messages(
        vec![Message {
            role: "user".to_string(),
            content: Some(text),
            tool_calls: None,
        }],
        None,
    );

    let response = specialist.execute(gpu, llm_messages, None, client).await?;
    let summary = response.content.unwrap_or_default();

    db.execute(
        "UPDATE conversations SET summary = ?1 WHERE id = ?2",
        rusqlite::params![summary, conversation_id]
    )?;

    Ok(summary)
}
