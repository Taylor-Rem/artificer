use std::sync::Arc;
use anyhow::Result;
use tokio::sync::watch;

use artificer_engine::api;
use artificer_engine::api::handlers::AppState;
use artificer_engine::background::Worker;
use artificer_engine::pool::GpuPool;
use artificer_shared::db;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    println!("Starting Artificer...\n");

    // Initialize database
    let db = db::init();

    // Initialize GPU pool from hardware.json
    let pool = Arc::new(GpuPool::load()?);

    // Build shared application state
    let state = AppState {
        db: db.clone(),
        pool: pool.clone(),
    };

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start background worker
    let worker_shutdown_rx = shutdown_rx.clone();
    let worker = Worker::new(db.clone(), pool.clone(), 2, worker_shutdown_rx);
    let worker_handle = tokio::spawn(async move {
        if let Err(e) = worker.run().await {
            eprintln!("Worker crashed: {}", e);
        }
        worker
    });

    // Start API server
    let api_shutdown_rx = shutdown_rx.clone();
    let api_handle = tokio::spawn(async move {
        if let Err(e) = api::start_server(state, api_shutdown_rx).await {
            eprintln!("API server crashed: {}", e);
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    println!("\nReceived shutdown signal...");

    let _ = shutdown_tx.send(true);
    let _ = api_handle.await;
    let worker = worker_handle.await?;
    worker.drain_queue().await?;

    println!("Artificer shutdown complete.");
    Ok(())
}
