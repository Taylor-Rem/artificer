use std::sync::Arc;
use axum::{
    extract::{Extension, Json},
    response::{IntoResponse, Response, Sse},
    http::StatusCode,
};
use futures_util::stream::StreamExt;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use artificer_shared::db::Db;
use crate::agent::AgentContext;
use crate::api::events::{EventSender, SseEvent};
use crate::api::types::{
    ChatRequest,
    RegisterDeviceRequest, RegisterDeviceResponse,
};
use crate::pool::AgentPool;
use crate::pool::gpu_pool::GpuPool;

// ============================================================================
// APP STATE
// ============================================================================

/// Shared application state injected into every handler via Extension.
#[derive(Clone)]
pub struct AppState {
    pub gpu_pool: Arc<GpuPool>,
    pub agent_pool: Arc<AgentPool>,
}

// ============================================================================
// ERROR TYPE
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(tag = "error_type")]
pub enum ApiError {
    Authentication { message: String },
    NotFound { message: String, resource: String },
    InvalidRequest { message: String, field: Option<String> },
    ResourceBusy { message: String },
    InternalError { message: String },
}

impl ApiError {
    fn to_response(self) -> Response {
        let (status, body) = match self {
            ApiError::Authentication { message } => (
                StatusCode::UNAUTHORIZED,
                serde_json::json!({ "error": message, "type": "authentication" }),
            ),
            ApiError::NotFound { message, resource } => (
                StatusCode::NOT_FOUND,
                serde_json::json!({ "error": message, "resource": resource, "type": "not_found" }),
            ),
            ApiError::InvalidRequest { message, field } => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({ "error": message, "field": field, "type": "invalid_request" }),
            ),
            ApiError::ResourceBusy { message } => (
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({ "error": message, "type": "resource_busy" }),
            ),
            ApiError::InternalError { message } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": message, "type": "internal_error" }),
            ),
        };
        (status, Json(body)).into_response()
    }
}

// ============================================================================
// HANDLERS
// ============================================================================

/// POST /chat
pub async fn handle_chat(
    Extension(state): Extension<AppState>,
    Json(req): Json<ChatRequest>,
) -> Response {
    // Validate request
    if let Err(e) = validate_chat_request(&req) {
        return e.to_response();
    }

    // Authenticate device
    let device_id = match authenticate_device(state.agent_pool.db(), &req.device_key) {
        Ok(id) => {
            println!("Device {} authenticated", id);
            id
        }
        Err(e) => return ApiError::Authentication {
            message: format!("Invalid device key: {}", e),
        }.to_response(),
    };

    // Resolve conversation
    let conversation_id = match resolve_conversation(state.agent_pool.db(), device_id, req.conversation_id) {
        Ok(id) => {
            println!("Using conversation {} for device {}", id, device_id);
            id
        }
        Err(e) => return ApiError::InternalError {
            message: format!("Failed to create/retrieve conversation: {}", e),
        }.to_response(),
    };

    // Acquire GPU
    let gpu = match state.gpu_pool.acquire_interactive() {
        Some(gpu) => {
            println!("GPU {} acquired for conversation {}", gpu.id, conversation_id);
            gpu
        }
        None => {
            eprintln!("No GPUs available for conversation {}", conversation_id);
            return ApiError::ResourceBusy {
                message: "All GPUs are currently busy processing other requests. Please try again in a moment.".to_string(),
            }.to_response();
        }
    };
    let gpu_id = gpu.id.clone();

    // Set up SSE channel
    let (tx, rx) = mpsc::channel::<SseEvent>(32);
    let events = EventSender::new(tx);

    let gpu_pool = state.gpu_pool.clone();
    let agent_pool = state.agent_pool.clone();

    tokio::spawn(async move {
        let context = AgentContext {
            device_id,
            device_key: req.device_key.clone(),
            conversation_id,
            parent_task_id: None,
            gpu,
            events: Some(events.clone()),
            synthesize: false,
        };

        // Get orchestrator and execute
        match agent_pool.get("Orchestrator") {
            Some(orchestrator) => {
                let execution = crate::agent::AgentExecution::new(
                    orchestrator,
                    context,
                    &req.message,
                    &agent_pool,
                );
                match execution.execute(agent_pool.clone()).await {
                    Ok(_) => {
                        // Success — response already streamed via events
                    }
                    Err(e) => {
                        events.error(&e.to_string());
                    }
                }
            }
            None => {
                events.error("Orchestrator agent not found");
            }
        }

        gpu_pool.release(&gpu_id);

        // Queue title generation after the first exchange
        let message_count = agent_pool.db()
            .get_message_count(conversation_id)
            .unwrap_or(0);

        if message_count <= 2 {
            let _ = agent_pool.db().queue_title_generation(
                device_id as i64,
                conversation_id,
                &req.message,
            );
        }

        events.done(conversation_id);
    });

    let stream = ReceiverStream::new(rx).map(|event| event.to_sse());
    Sse::new(stream).into_response()
}

/// POST /devices/register
pub async fn handle_register_device(
    Extension(state): Extension<AppState>,
    Json(req): Json<RegisterDeviceRequest>,
) -> Response {
    let device_key = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let conn = match state.agent_pool.db().lock() {
        Ok(c) => c,
        Err(e) => return ApiError::InternalError {
            message: format!("Database unavailable: {}", e),
        }.to_response(),
    };

    let result = conn.execute(
        "INSERT INTO devices (device_name, device_key, active, created, last_seen)
         VALUES (?1, ?2, 1, ?3, ?4)
         ON CONFLICT(device_name) DO UPDATE SET
           device_key = excluded.device_key,
           active = 1,
           last_seen = excluded.last_seen",
        rusqlite::params![req.device_name, device_key, now, now],
    );

    if let Err(e) = result {
        return ApiError::InternalError {
            message: format!("Failed to register device: {}", e),
        }.to_response();
    }

    let device_id: i64 = match conn.query_row(
        "SELECT id FROM devices WHERE device_name = ?1",
        rusqlite::params![req.device_name],
        |row| row.get(0),
    ) {
        Ok(id) => id,
        Err(e) => return ApiError::InternalError {
            message: format!("Failed to retrieve device id: {}", e),
        }.to_response(),
    };

    println!("Device registered: '{}' (id={})", req.device_name, device_id);

    Json(RegisterDeviceResponse {
        device_id,
        device_key,
    }).into_response()
}

/// POST /devices/verify
pub async fn handle_verify_device(
    Extension(state): Extension<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let device_id = match body["device_id"].as_i64() {
        Some(id) => id,
        None => return ApiError::InvalidRequest {
            message: "Missing device_id".to_string(),
            field: Some("device_id".to_string()),
        }.to_response(),
    };

    let device_key = match body["device_key"].as_str() {
        Some(k) => k.to_string(),
        None => return ApiError::InvalidRequest {
            message: "Missing device_key".to_string(),
            field: Some("device_key".to_string()),
        }.to_response(),
    };

    let conn = match state.agent_pool.db().lock() {
        Ok(c) => c,
        Err(e) => return ApiError::InternalError {
            message: format!("Database unavailable: {}", e),
        }.to_response(),
    };

    let valid = conn.query_row(
        "SELECT 1 FROM devices WHERE id = ?1 AND device_key = ?2 AND active = 1",
        rusqlite::params![device_id, device_key],
        |_| Ok(true),
    ).unwrap_or(false);

    if !valid {
        return ApiError::Authentication {
            message: "Invalid or inactive device credentials".to_string(),
        }.to_response();
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let _ = conn.execute(
        "UPDATE devices SET last_seen = ?1 WHERE id = ?2",
        rusqlite::params![now, device_id],
    );

    StatusCode::OK.into_response()
}

/// GET /status
pub async fn handle_status(
    Extension(state): Extension<AppState>,
) -> impl IntoResponse {
    let gpu_status = state.gpu_pool.status();
    Json(serde_json::json!({
        "status": "ok",
        "gpus": gpu_status,
    }))
}

/// GET /background/status
pub async fn handle_background_status(
    Extension(state): Extension<AppState>,
) -> Response {
    let conn = match state.agent_pool.db().lock() {
        Ok(c) => c,
        Err(e) => return ApiError::InternalError {
            message: format!("Database unavailable: {}", e),
        }.to_response(),
    };

    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM background WHERE status = 'pending'",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let running: i64 = conn.query_row(
        "SELECT COUNT(*) FROM background WHERE status = 'running'",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let failed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM background WHERE status = 'failed'",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    let completed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM background WHERE status = 'completed'",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    Json(serde_json::json!({
        "pending": pending,
        "running": running,
        "failed": failed,
        "completed": completed,
    })).into_response()
}

// ============================================================================
// HELPERS
// ============================================================================

fn validate_chat_request(req: &ChatRequest) -> Result<(), ApiError> {
    if req.message.trim().is_empty() {
        return Err(ApiError::InvalidRequest {
            message: "Message cannot be empty".to_string(),
            field: Some("message".to_string()),
        });
    }

    if req.message.len() > 50_000 {
        return Err(ApiError::InvalidRequest {
            message: "Message too long (max 50,000 characters)".to_string(),
            field: Some("message".to_string()),
        });
    }

    if req.device_key.is_empty() {
        return Err(ApiError::InvalidRequest {
            message: "Device key required".to_string(),
            field: Some("device_key".to_string()),
        });
    }

    Ok(())
}

fn authenticate_device(db: &Db, device_key: &str) -> anyhow::Result<u64> {
    // Check if the device exists at all (active or not)
    let active_status: Option<bool> = db.query_row_optional(
        "SELECT active FROM devices WHERE device_key = ?1",
        rusqlite::params![device_key],
        |row| row.get(0),
    )?;

    match active_status {
        Some(false) => Err(anyhow::anyhow!("Device is deactivated. Contact administrator.")),
        Some(true) => {
            db.query_row_optional(
                "SELECT id FROM devices WHERE device_key = ?1 AND active = 1",
                rusqlite::params![device_key],
                |row| row.get(0),
            )?
            .ok_or_else(|| anyhow::anyhow!("Unexpected authentication error"))
        }
        None => Err(anyhow::anyhow!("Invalid device key. Please register your device.")),
    }
}

fn resolve_conversation(
    db: &Db,
    device_id: u64,
    existing_id: Option<u64>,
) -> anyhow::Result<u64> {
    match existing_id {
        Some(id) => Ok(id),
        None => db.create_conversation(device_id),
    }
}
