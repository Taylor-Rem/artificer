use axum::{
    routing::{get, post},
    Router,
};
use super::handlers;

pub fn create_router() -> Router {
    Router::new()
        .route("/chat", post(handlers::handle_chat))
        .route("/status", get(handlers::handle_status))
}
