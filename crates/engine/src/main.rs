use anyhow::Result;
use tokio::sync::watch;

use artificer_engine::api;
use artificer_engine::task::worker::Worker;
use artificer_tools::db;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting Artificer...\n");

    // Initialize database
    let db = db::init();

    // Register tasks
    use artificer_engine::task::Task;
    for task in Task::all() {
        db.register_task(task.task_id(), task.title(), task.description())?;
    }

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start worker
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

    let _ = shutdown_tx.send(true);
    let _ = api_handle.await;
    let worker = worker_handle.await?;
    worker.drain_queue().await?;

    println!("Artificer shutdown complete.");
    Ok(())
}
