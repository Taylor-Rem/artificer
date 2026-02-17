use anyhow::Result;
use axum::{extract::Json, http::StatusCode, response::IntoResponse, routing::post, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use artificer_tools::registry;
use std::sync::Arc;

struct ToolServerState {
    device_id: i64,
    device_key: String,
}

#[derive(Deserialize)]
struct ToolExecutionRequest {
    device_id: i64,
    device_key: String,
    tool_name: String,
    arguments: Value,
}

async fn handle_tool_execution(
    state: axum::extract::State<Arc<ToolServerState>>,
    Json(req): Json<ToolExecutionRequest>,
) -> impl IntoResponse {
    // Validate device credentials
    if req.device_id != state.device_id || req.device_key != state.device_key {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid device credentials" })),
        );
    }

    match registry::use_tool(&req.tool_name, &req.arguments) {
        Ok(result) => (StatusCode::OK, Json(json!({ "result": result }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Tool execution failed: {}", e) })),
        ),
    }
}

pub async fn start_tool_server(device_id: i64, device_key: String) -> Result<()> {
    let state = Arc::new(ToolServerState {
        device_id,
        device_key,
    });

    let app = Router::new()
        .route("/tools/execute", post(handle_tool_execution))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081").await?;
    // println!("Tool server listening on port 8081");
    axum::serve(listener, app).await?;

    Ok(())
}
