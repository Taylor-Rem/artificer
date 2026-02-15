// src/main.rs
use anyhow::Result;

use artificer::task::{Task, worker::Worker};

#[tokio::main]
async fn main() -> Result<()> {
    // Start background worker for async jobs
    let worker = Worker::new(2);
    tokio::spawn(async move {
        if let Err(e) = worker.run().await {
            eprintln!("Worker crashed: {}", e);
        }
    });

    println!("Artificer is ready. Type 'quit' to exit.\n");

    Task::Chat.start_interactive_session().await?;

    Ok(())
}