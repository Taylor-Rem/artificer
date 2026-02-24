use anyhow::Result;
use axum::extract::Extension;
use tokio::sync::watch;

use super::handlers::AppState;
use super::routes::create_router;

pub async fn start_server(state: AppState, shutdown_rx: watch::Receiver<bool>) -> Result<()> {
    let app = create_router()
        .layer(Extension(state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    println!("Artificer API server listening on http://0.0.0.0:8080");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_rx))
        .await?;

    Ok(())
}

async fn shutdown_signal(mut shutdown_rx: watch::Receiver<bool>) {
    while !*shutdown_rx.borrow() {
        if shutdown_rx.changed().await.is_err() {
            break;
        }
    }
    println!("Shutting down API server...");
}
