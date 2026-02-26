use std::sync::Arc;
use anyhow::Result;
use tokio::sync::watch;

use artificer_engine::api;
use artificer_engine::api::handlers::AppState;
use artificer_engine::background::Worker;
use artificer_engine::pool::{GpuPool, AgentPool};
use artificer_shared::db;
use artificer_shared::executor::ToolExecutor;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    println!("╔════════════════════════════════════════╗");
    println!("║        ARTIFICER STARTING UP           ║");
    println!("╚════════════════════════════════════════╝");
    println!();

    // Initialize database
    println!("→ Initializing database...");
    let db = db::init();
    println!("  ✓ Database initialized");

    // Initialize GPU pool from hardware.json
    println!("→ Loading GPU configuration...");
    let gpu_pool = match GpuPool::load() {
        Ok(pool) => {
            println!("  ✓ GPU pool loaded");
            Arc::new(pool)
        }
        Err(e) => {
            eprintln!("  ✗ Failed to load GPU pool: {}", e);
            eprintln!();
            eprintln!("Make sure hardware.json exists in the workspace root.");
            return Err(e);
        }
    };

    // Initialize tool executor
    println!("→ Configuring tool executor...");
    let envoy_url = std::env::var("ENVOY_URL")
        .ok()
        .or_else(|| Some("http://localhost:8081".to_string()));

    if let Some(ref url) = envoy_url {
        println!("  ✓ Envoy URL: {}", url);
    } else {
        println!("  ⚠ No envoy configured (client tools disabled)");
    }
    let tool_executor = Arc::new(ToolExecutor::new(envoy_url));

    // Initialize agent pool with shared resources
    println!("→ Building agent pool...");
    let agent_pool = Arc::new(AgentPool::new(db.clone(), tool_executor));
    println!("  ✓ Agent pool ready");

    // Build shared application state
    let state = AppState {
        gpu_pool: gpu_pool.clone(),
        agent_pool: agent_pool.clone(),
    };

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start background worker
    println!("→ Starting background worker...");
    let worker_shutdown_rx = shutdown_rx.clone();
    let worker = Worker::new(agent_pool.clone(), gpu_pool.clone(), 2, worker_shutdown_rx);
    let worker_handle = tokio::spawn(async move {
        if let Err(e) = worker.run().await {
            eprintln!("Worker crashed: {}", e);
        }
        worker
    });
    println!("  ✓ Background worker started");

    // Start API server
    println!("→ Starting API server...");
    let api_shutdown_rx = shutdown_rx.clone();
    let api_handle = tokio::spawn(async move {
        if let Err(e) = api::start_server(state, api_shutdown_rx).await {
            eprintln!("API server crashed: {}", e);
        }
    });

    println!();
    println!("╔════════════════════════════════════════╗");
    println!("║     ARTIFICER READY FOR REQUESTS       ║");
    println!("╚════════════════════════════════════════╝");
    println!();
    println!("API server: http://0.0.0.0:8080");
    println!("Press Ctrl+C to shutdown gracefully");
    println!();

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;

    println!();
    println!("╔════════════════════════════════════════╗");
    println!("║       SHUTTING DOWN GRACEFULLY         ║");
    println!("╚════════════════════════════════════════╝");
    println!();

    let _ = shutdown_tx.send(true);

    println!("→ Stopping API server...");
    let _ = api_handle.await;
    println!("  ✓ API server stopped");

    println!("→ Draining background job queue...");
    let worker = worker_handle.await?;
    worker.drain_queue().await?;
    println!("  ✓ Background jobs complete");

    println!();
    println!("Artificer shutdown complete. Goodbye!");
    Ok(())
}
