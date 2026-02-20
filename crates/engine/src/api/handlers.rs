use axum::{
    extract::Json,
    http::StatusCode,
    response::{IntoResponse, sse::{Event, Sse}},
};
use futures_util::stream::Stream;
use serde_json::json;
use axum::extract::State;
use std::sync::Arc;
use std::convert::Infallible;
use artificer_shared::{db::Db, rusqlite};
use crate::events;
use crate::task::{conversation::Conversation, PipelineStep, Task};
use crate::Message;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use super::types::*;

pub async fn handle_chat(
    State(db): State<Arc<Db>>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Generate unique request ID
    let request_id = uuid::Uuid::new_v4().to_string();

    // Create event receiver
    let rx = events::create_channel(request_id.clone());

    tokio::spawn(async move {
        let conversation = Conversation::new(req.device_id);
        let events = events::EventSender::new(request_id.clone());

        let conversation_id = match req.conversation_id {
            Some(id) => id,
            None => {
                let user_message = Message {
                    role: "user".to_string(),
                    content: Some(req.message.clone()),
                    tool_calls: None,
                };
                match Conversation::new(req.device_id).init(&user_message, &Task::Router).await {
                    Ok((conv_id, _)) => conv_id,
                    Err(e) => {
                        events.error(format!("Failed to initialize conversation: {}", e));
                        return;
                    }
                }
            }
        };

        let mut messages = conversation.get_messages(conversation_id).unwrap_or_default();
        let mut message_count = messages.len() as u32;

        if let Err(e) = conversation.add_message(Some(conversation_id), "user", &req.message, &mut message_count) {
            eprintln!("Warning: Failed to save user message: {}", e);
        }

        messages.push(Message {
            role: "user".to_string(),
            content: Some(req.message.clone()),
            tool_calls: None,
        });

        // Run router to get pipeline plan
        let router_response = Task::Router.execute_with_prompt(
            vec![Message {
                role: "user".to_string(),
                content: Some(req.message.clone()),
                tool_calls: None,
            }],
            &db,
            req.device_id,
            req.device_key.clone(),
            false, // router doesn't stream
            None,
        ).await;

        // Parse pipeline steps from router tool call
        let steps: Vec<PipelineStep> = match router_response {
            Ok(response) => {
                // Router should have called plan_tasks — extract steps from tool call args
                if let Some(tool_calls) = &response.tool_calls {
                    if let Some(call) = tool_calls.first() {
                        serde_json::from_value(call.function.arguments["steps"].clone())
                            .unwrap_or_else(|_| default_chat_step(&req.message))
                    } else {
                        default_chat_step(&req.message)
                    }
                } else {
                    default_chat_step(&req.message)
                }
            }
            Err(e) => {
                eprintln!("Router failed: {}", e);
                default_chat_step(&req.message)
            }
        };

        // Execute the pipeline
        match Task::execute_pipeline(
            steps,
            &db,
            req.device_id,
            req.device_key.clone(),
            Some(events.clone()),
        ).await {
            Ok(response) => {
                if let Some(content) = &response.content {
                    if let Err(e) = conversation.add_message(Some(conversation_id), "assistant", content, &mut message_count) {
                        eprintln!("Warning: Failed to save assistant message: {}", e);
                    }
                }
                events.complete(conversation_id);
            }
            Err(e) => {
                events.error(format!("Pipeline execution failed: {}", e));
            }
        }
    });

    // Convert broadcast receiver to SSE stream
    let stream = BroadcastStream::new(rx)
        .filter_map(|result| {
            match result {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap();
                    Some(Ok(Event::default().data(json)))
                }
                Err(_) => None,
            }
        });

    Sse::new(stream)
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

pub async fn handle_tool_execution(Json(req): Json<ToolExecutionRequest>) -> impl IntoResponse {
    match artificer_shared::use_tool(&req.tool_name, &req.arguments) {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({ "result": result }))
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Tool execution failed: {}", e) }))
        ),
    }
}
fn default_chat_step(message: &str) -> Vec<PipelineStep> {
    vec![PipelineStep {
        task: "chat".to_string(),
        directions: message.to_string(),
    }]
}
pub async fn handle_verify_device() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({ "valid": true })))
}

pub async fn health_check() -> &'static str {
    "Artificer is running"
}