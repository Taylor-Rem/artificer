use std::sync::Arc;
use anyhow::Result;
use tokio::sync::watch;

use artificer_engine::api;
use artificer_engine::memory::Db;
use artificer_engine::task::worker::Worker;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting Artificer...\n");

    // Initialize database and inject into tools crate
    let db = Db::default();
    artificer_tools::db::set_database(Arc::new(db));

    // Create shutdown channel (shared between API server and worker)
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start background worker
    let worker_shutdown_rx = shutdown_rx.clone();
    let worker = Worker::new(2, worker_shutdown_rx);
    let worker_handle = tokio::spawn(async move {
        if let Err(e) = worker.run().await {
            eprintln!("Worker crashed: {}", e);
        }
        worker
    });

    // Start API server
    let api_shutdown_rx = shutdown_rx.clone();
    let api_handle = tokio::spawn(async move {
        if let Err(e) = api::start_server(api_shutdown_rx).await {
            eprintln!("API server crashed: {}", e);
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    println!("\nReceived shutdown signal...");

    // Signal shutdown to both server and worker
    let _ = shutdown_tx.send(true);

    // Wait for API server to stop
    let _ = api_handle.await;

    // Wait for worker to finish and drain queue
    let worker = worker_handle.await?;
    worker.drain_queue().await?;

    println!("Artificer shutdown complete.");
    Ok(())
}
