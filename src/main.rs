use anyhow::Result;
use tokio::sync::watch;

use artificer::task::{Task, worker::Worker};
use artificer::state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    let state = AppState::new();

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start background worker
    let worker = Worker::new(2, shutdown_rx);
    let worker_handle = tokio::spawn(async move {
        if let Err(e) = worker.run().await {
            eprintln!("Worker crashed: {}", e);
        }
        worker
    });

    println!("Artificer is ready. Type 'quit' to exit.\n");

    // Run interactive session
    Task::Chat.start_interactive_session(state).await?;

    // Signal worker to stop accepting new jobs
    println!("\nShutting down...");
    let _ = shutdown_tx.send(true);

    // Wait for worker to finish current job and get it back
    let worker = worker_handle.await?;

    // Process all remaining jobs
    worker.drain_queue().await?;

    println!("Goodbye!");
    Ok(())
}