use axum::{
    routing::{get, post},
    Router,
};
use super::handlers;

pub fn create_router() -> Router {
    Router::new()
        .route("/chat", post(handlers::handle_chat))
        .route("/status", get(handlers::handle_status))
        .route("/background/status", get(handlers::handle_background_status))
        .route("/devices/register", post(handlers::handle_register_device))
        .route("/devices/verify", post(handlers::handle_verify_device))
}