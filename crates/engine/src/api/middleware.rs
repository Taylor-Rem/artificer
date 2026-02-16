use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
    http::StatusCode,
    body::Body,
};
use std::sync::Arc;
use artificer_tools::db::Db;
use serde_json::json;

pub async fn authenticate_device(
    db: Arc<Db>,
    req: Request,
    next: Next,
) -> Response {
    // Extract body to read device_id and device_key
    let (parts, body) = req.into_parts();

    // Read body
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({ "error": "Invalid request body" }))
            ).into_response();
        }
    };

    // Parse JSON
    let json_value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({ "error": "Invalid JSON" }))
            ).into_response();
        }
    };

    // Extract credentials
    let device_id = match json_value.get("device_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({ "error": "Missing device_id" }))
            ).into_response();
        }
    };

    let device_key = match json_value.get("device_key").and_then(|v| v.as_str()) {
        Some(key) => key.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({ "error": "Missing device_key" }))
            ).into_response();
        }
    };

    // Verify device and update last_seen — scoped to drop MutexGuard before await
    {
        let conn = match db.lock() {
            Ok(c) => c,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({ "error": "Database error" }))
                ).into_response();
            }
        };

        let valid = conn.query_row(
            "SELECT 1 FROM devices WHERE id = ?1 AND device_key = ?2",
            rusqlite::params![device_id, &device_key],
            |_| Ok(true)
        ).unwrap_or(false);

        if !valid {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({
                    "error": "Invalid device credentials",
                    "code": "DEVICE_NOT_FOUND"
                }))
            ).into_response();
        }

        // Update last_seen
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let _ = conn.execute(
            "UPDATE devices SET last_seen = ?1 WHERE id = ?2",
            rusqlite::params![now, device_id],
        );
    }

    // Reconstruct request with original body
    let req = Request::from_parts(parts, Body::from(bytes));

    // Continue to handler
    next.run(req).await
}
