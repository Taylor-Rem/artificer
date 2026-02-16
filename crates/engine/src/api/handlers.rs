use axum::{extract::Json, http::StatusCode, response::IntoResponse};
use serde_json::json;
use axum::extract::State;
use std::sync::Arc;
use artificer_tools::db::Db;
use artificer_tools::rusqlite;
use crate::task::{conversation::Conversation, Task};
use crate::Message;

use super::types::*;

pub async fn handle_chat(
    State(db): State<Arc<Db>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let conversation = Conversation::new(req.device_id);
    let task = Task::Chat;

    // Initialize conversation if needed
    let conversation_id = match req.conversation_id {
        Some(id) => id,
        None => {
            let user_message = Message {
                role: "user".to_string(),
                content: Some(req.message.clone()),
                tool_calls: None,
            };

            match conversation.init(&user_message, &task).await {
                Ok((conv_id, _th_id)) => conv_id,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": format!("Failed to initialize conversation: {}", e) })),
                    );
                }
            }
        }
    };

    // Build message for execution
    let user_message = Message {
        role: "user".to_string(),
        content: Some(req.message.clone()),
        tool_calls: None,
    };

    // Save user message
    let mut message_count = 0;
    if let Err(e) = conversation.add_message(Some(conversation_id), "user", &req.message, &mut message_count) {
        eprintln!("Warning: Failed to save user message: {}", e);
    }

    // Execute task
    let messages = vec![user_message];
    let response = match task.execute_with_prompt(messages, &db, req.device_id, false).await {
        Ok(resp) => resp,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Task execution failed: {}", e) })),
            );
        }
    };

    // Save assistant response
    if let Some(content) = &response.content {
        if let Err(e) = conversation.add_message(Some(conversation_id), "assistant", content, &mut message_count) {
            eprintln!("Warning: Failed to save assistant message: {}", e);
        }
    }

    let chat_response = ChatResponse {
        conversation_id,
        content: response.content.unwrap_or_default(),
    };

    (StatusCode::OK, Json(serde_json::to_value(chat_response).unwrap()))
}

pub async fn handle_register_device(
    State(db): State<Arc<Db>>,
    Json(req): Json<RegisterDeviceRequest>,
) -> impl IntoResponse {
    let conn = match db.lock() {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Database error: {}", e) })),
            );
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Check if device exists by name
    match conn.query_row(
        "SELECT id, device_key FROM devices WHERE device_name = ?1",
        rusqlite::params![req.device_name],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    ) {
        Ok((device_id, device_key)) => {
            // Update last_seen
            let _ = conn.execute(
                "UPDATE devices SET last_seen = ?1 WHERE id = ?2",
                rusqlite::params![now, device_id],
            );

            (StatusCode::OK, Json(json!({
                "device_id": device_id,
                "device_key": device_key
            })))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // Generate secure random key
            use uuid::Uuid;
            let device_key = Uuid::new_v4().to_string();

            let metadata = json!({
                "registered_via": "api",
            });

            match conn.execute(
                "INSERT INTO devices (device_name, device_key, created, last_seen, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![req.device_name, device_key, now, now, metadata.to_string()],
            ) {
                Ok(_) => {
                    let device_id = conn.last_insert_rowid();
                    (StatusCode::OK, Json(json!({
                        "device_id": device_id,
                        "device_key": device_key
                    })))
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("Failed to create device: {}", e) })),
                ),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Database error: {}", e) })),
        ),
    }
}
pub async fn handle_list_conversations(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let device_id: i64 = match params.get("device_id").and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Missing or invalid device_id parameter" })),
            );
        }
    };

    let db = Db::default();
    let conn = match db.lock() {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Database error: {}", e) })),
            );
        }
    };

    let mut stmt = match conn.prepare(
        "SELECT id, title, created, last_accessed FROM conversations
         WHERE device_id = ?1
         ORDER BY last_accessed DESC",
    ) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Query error: {}", e) })),
            );
        }
    };

    let conversations: Vec<ConversationInfo> = stmt
        .query_map(rusqlite::params![device_id], |row| {
            Ok(ConversationInfo {
                id: row.get(0)?,
                title: row.get(1)?,
                created: row.get(2)?,
                last_accessed: row.get(3)?,
            })
        })
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    (StatusCode::OK, Json(serde_json::to_value(ListConversationsResponse { conversations }).unwrap()))
}
// crates/engine/src/api/handlers.rs

pub async fn handle_queue_summarization(Json(req): Json<QueueJobRequest>) -> impl IntoResponse {
    let conversation = Conversation::new(req.device_id);

    match conversation.summarize(req.conversation_id) {
        Ok(job_id) => (
            StatusCode::OK,
            Json(json!({ "job_id": job_id, "status": "queued" }))
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to queue job: {}", e) }))
        ),
    }
}

pub async fn handle_queue_memory_extraction(Json(req): Json<QueueJobRequest>) -> impl IntoResponse {
    let conversation = Conversation::new(req.device_id);

    match conversation.extract_memory(req.conversation_id) {
        Ok(job_id) => (
            StatusCode::OK,
            Json(json!({ "job_id": job_id, "status": "queued" }))
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to queue job: {}", e) }))
        ),
    }
}
pub async fn health_check() -> &'static str {
    "Artificer is running"
}