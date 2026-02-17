use axum::{
    extract::Request,
    middleware::{self, Next},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use super::{handlers, middleware as api_middleware};
use artificer_tools::db::Db;

pub fn create_router(db: Arc<Db>) -> Router<Arc<Db>> {
    let protected = Router::new()
        .route("/chat", post(handlers::handle_chat))
        .route("/conversations", get(handlers::handle_list_conversations))
        .route("/jobs/summarize", post(handlers::handle_queue_summarization))
        .route("/jobs/extract_memory", post(handlers::handle_queue_memory_extraction))
        .route("/tools/execute", post(handlers::handle_tool_execution))
        .route_layer(middleware::from_fn(move |req: Request, next: Next| {
            let db = db.clone();
            async move {
                api_middleware::authenticate_device(db, req, next).await
            }
        }));

    Router::new()
        .route("/", get(handlers::health_check))
        .route("/devices/register", post(handlers::handle_register_device))
        .merge(protected)
}