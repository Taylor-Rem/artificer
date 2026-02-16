use axum::{
    routing::{get, post},
    Router,
};

use super::handlers;

pub fn create_router() -> Router {
    Router::new()
        .route("/", get(handlers::health_check))
        .route("/chat", post(handlers::handle_chat))
        .route("/devices/register", post(handlers::handle_register_device))
        .route("/conversations", get(handlers::handle_list_conversations))
}