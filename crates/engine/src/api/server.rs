use anyhow::Result;
use tokio::sync::watch;
use std::sync::Arc;
use artificer_tools::db::Db;

use super::routes::create_router;

pub async fn start_server(shutdown_rx: watch::Receiver<bool>) -> Result<()> {
    let db = Arc::new(Db::default());

    let app = create_router()
        .with_state(db);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    println!("🚀 Artificer API server listening on http://0.0.0.0:8080");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_rx))
        .await?;

    Ok(())
}

async fn shutdown_signal(mut shutdown_rx: watch::Receiver<bool>) {
    // Wait for shutdown signal
    while !*shutdown_rx.borrow() {
        if shutdown_rx.changed().await.is_err() {
            break;
        }
    }
    println!("Shutting down API server...");
}