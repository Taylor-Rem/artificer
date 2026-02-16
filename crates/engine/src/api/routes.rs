use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use super::handlers;
use crate::memory::Db;

pub fn create_router() -> Router<Arc<Db>> {
    Router::new()
        .route("/", get(handlers::health_check))
        .route("/chat", post(handlers::handle_chat))
        .route("/devices/register", post(handlers::handle_register_device))
        .route("/conversations", get(handlers::handle_list_conversations))
        .route("/jobs/summarize", post(handlers::handle_queue_summarization))
        .route("/jobs/extract_memory", post(handlers::handle_queue_memory_extraction))
}