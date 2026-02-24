use std::sync::Arc;
use axum::{
    extract::{Extension, Json},
    response::{IntoResponse, Response, Sse},
    http::StatusCode,
};
use futures_util::stream::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use artificer_shared::db::Db;
use crate::api::events::{EventSender, SseEvent};
use crate::api::types::{ChatRequest, ChatResponse, ErrorResponse};
use crate::orchestrator::Orchestrator;
use crate::pool::GpuPool;

/// Shared application state injected into every handler via Extension.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub pool: Arc<GpuPool>,
}

/// POST /chat
///
/// Entry point for all user requests. Responsibilities:
///   1. Authenticate the device
///   2. Resolve or create the conversation
///   3. Persist the user message
///   4. Acquire a GPU
///   5. Hand off to the Orchestrator
///   6. Release the GPU
///   7. Stream or return the response
pub async fn handle_chat(
    Extension(state): Extension<AppState>,
    Json(req): Json<ChatRequest>,
) -> Response {
    // --- Authenticate device ---
    let device_id = match authenticate_device(&state.db, &req.device_key) {
        Ok(id) => id,
        Err(e) => return error_response(StatusCode::UNAUTHORIZED, &e.to_string()),
    };

    // --- Resolve conversation ---
    let conversation_id = match resolve_conversation(&state.db, device_id, req.conversation_id) {
        Ok(id) => id,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    // --- Load message count for this conversation (for ordered inserts) ---
    let message_count = match state.db.get_message_count(conversation_id) {
        Ok(n) => n,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let mut message_count = message_count;

    // --- Load conversation history ---
    let history = match state.db.get_messages(conversation_id) {
        Ok(msgs) => msgs,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    // --- Persist the user message ---
    // We do this before acquiring the GPU so the message is always recorded,
    // even if GPU acquisition fails.
    let is_first_message = history.is_empty();
    if let Err(e) = state.db.add_message(
        conversation_id,
        None, // task_id not known yet — the Orchestrator creates it
        "user",
        Some(&req.message),
        None,
        &mut message_count,
    ) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
    }

    // --- Acquire GPU ---
    let gpu = match state.pool.acquire_interactive() {
        Some(gpu) => gpu,
        None => return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "All GPUs are currently busy. Try again shortly.",
        ),
    };
    let gpu_id = gpu.id.clone();

    // --- Stream or non-stream path ---
    if req.stream.unwrap_or(true) {
        // Set up SSE channel
        let (tx, rx) = mpsc::channel::<SseEvent>(32);
        let events = EventSender::new(tx);

        let db = state.db.clone();
        let pool = state.pool.clone();
        let goal = req.message.clone();

        tokio::spawn(async move {
            let orchestrator = Orchestrator::new(
                db.clone(),
                gpu,
                device_id,
                Some(events.clone()),
            );

            let result = orchestrator.run(
                goal.clone(),
                conversation_id,
                history,
                message_count,
            ).await;

            // Release GPU before queuing anything else
            pool.release(&gpu_id);

            // Queue background jobs if this was the first message
            // (title generation only makes sense once there's content)
            if is_first_message {
                let _ = db.queue_conversation_jobs(device_id, conversation_id, &goal);
            }

            if let Err(e) = result {
                events.error(&e.to_string());
            }

            events.done();
        });

        let stream = ReceiverStream::new(rx).map(|event| event.to_sse());
        Sse::new(stream).into_response()

    } else {
        // Non-streaming: run synchronously, return JSON
        let orchestrator = Orchestrator::new(
            state.db.clone(),
            gpu,
            device_id,
            None,
        );

        let result = orchestrator.run(
            req.message.clone(),
            conversation_id,
            history,
            message_count,
        ).await;

        state.pool.release(&gpu_id);

        if is_first_message {
            let _ = state.db.queue_conversation_jobs(
                device_id,
                conversation_id,
                &req.message,
            );
        }

        match result {
            Ok(content) => Json(ChatResponse {
                conversation_id,
                content,
            }).into_response(),
            Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
        }
    }
}

/// GET /status
pub async fn handle_status(
    Extension(state): Extension<AppState>,
) -> impl IntoResponse {
    let gpu_status = state.pool.status();
    Json(serde_json::json!({
        "status": "ok",
        "gpus": gpu_status,
    }))
}

// ============================================================================
// HELPERS
// ============================================================================

fn authenticate_device(db: &Db, device_key: &str) -> anyhow::Result<i64> {
    db.query_row_optional(
        "SELECT id FROM devices WHERE device_key = ?1 AND active = 1",
        rusqlite::params![device_key],
        |row| row.get(0),
    )?
        .ok_or_else(|| anyhow::anyhow!("Invalid or inactive device key"))
}

fn resolve_conversation(
    db: &Db,
    device_id: i64,
    existing_id: Option<u64>,
) -> anyhow::Result<u64> {
    match existing_id {
        Some(id) => Ok(id),
        None => db.create_conversation(device_id),
    }
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(ErrorResponse { error: message.to_string() })).into_response()
}