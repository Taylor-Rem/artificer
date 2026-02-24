pub mod title_generation;
pub mod summarization;
pub(crate) mod memory_extraction;

use std::sync::Arc;
use anyhow::Result;
use reqwest::Client;
use tokio::time::{sleep, Duration};
use tokio::sync::watch;
use serde_json::Value;
use artificer_shared::{db::Db, rusqlite};

use crate::pool::GpuPool;

#[derive(Debug)]
struct PendingJob {
    id: i64,
    #[allow(dead_code)]
    device_id: Option<i64>,
    method: String,
    arguments: Value,
}

impl PendingJob {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let id = row.get(0)?;
        let device_id = row.get(1)?;
        let method_str: String = row.get(2)?;
        let arguments_str: String = row.get(3)?;

        let arguments = serde_json::from_str(&arguments_str)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;

        Ok(PendingJob { id, device_id, method: method_str, arguments })
    }
}

fn needs_context(method: &str) -> bool {
    matches!(method, "summarization" | "memory_extraction"
        | "task_summarization" | "task_memory_extraction")
}

pub struct Worker {
    db: Arc<Db>,
    pool: Arc<GpuPool>,
    client: Client,
    poll_interval: Duration,
    shutdown_rx: watch::Receiver<bool>,
}

impl Worker {
    pub fn new(
        db: Arc<Db>,
        pool: Arc<GpuPool>,
        poll_interval_secs: u64,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            db,
            pool,
            client: Client::new(),
            poll_interval: Duration::from_secs(poll_interval_secs),
            shutdown_rx,
        }
    }

    pub async fn run(&self) -> Result<()> {
        loop {
            if *self.shutdown_rx.borrow() {
                println!("Worker shutting down gracefully...");
                break;
            }

            if let Err(e) = self.process_next_job().await {
                eprintln!("Worker error: {}", e);
            }

            sleep(self.poll_interval).await;
        }

        Ok(())
    }

    /// Process all remaining jobs before shutdown
    pub async fn drain_queue(&self) -> Result<()> {
        println!("Processing remaining background jobs...");

        loop {
            let has_pending = self.has_pending_jobs()?;
            if !has_pending {
                break;
            }

            if let Err(e) = self.process_next_job().await {
                eprintln!("Error during drain: {}", e);
            }

            sleep(Duration::from_millis(100)).await;
        }

        println!("All background jobs completed.");
        Ok(())
    }

    fn has_pending_jobs(&self) -> Result<bool> {
        let conn = self.db.lock()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM background WHERE status IN ('pending', 'running')",
            [],
            |row| row.get(0)
        )?;
        Ok(count > 0)
    }

    async fn process_next_job(&self) -> Result<()> {
        let job = self.db.query_row_optional(
            "SELECT id, device_id, method, arguments FROM background
             WHERE status = 'pending'
             ORDER BY priority DESC, created_at ASC
             LIMIT 1",
            [],
            PendingJob::from_row
        )?;

        let Some(job) = job else {
            return Ok(());
        };

        // Acquire a background GPU — skip if none available
        let gpu = match self.pool.acquire_background() {
            Some(gpu) => gpu,
            None => return Ok(()), // try again next poll
        };
        let gpu_id = gpu.id.clone();

        self.mark_job_running(job.id)?;

        let messages = if needs_context(&job.method) {
            let conversation_id = job.arguments["conversation_id"].as_i64()
                .ok_or_else(|| anyhow::anyhow!(
                    "Missing conversation_id for context-requiring job '{}'",
                    job.method
                ))?;
            let json = self.db.query(
                "SELECT role, message FROM messages WHERE conversation_id = ?1 ORDER BY m_order",
                rusqlite::params![conversation_id],
            )?;
            Some(serde_json::from_str::<Vec<Value>>(&json)?)
        } else {
            None
        };

        let result = match job.method.as_str() {
            "title_generation" => {
                title_generation::execute(
                    &self.db, &gpu, &self.client, &job.arguments,
                ).await
            }
            "summarization" => {
                summarization::execute(
                    &self.db, &gpu, &self.client, &job.arguments, messages.as_deref(),
                ).await
            }
            "memory_extraction" => {
                memory_extraction::execute(
                    &self.db, &gpu, &self.client, &job.arguments, messages.as_deref(),
                ).await
            }
            other => Err(anyhow::anyhow!("Unknown job method: {}", other)),
        };

        // Always release the GPU
        self.pool.release(&gpu_id);

        match result {
            Ok(res) => self.mark_job_complete(job.id, &res)?,
            Err(e) => {
                let exhausted = self.mark_job_failed(job.id, &e.to_string())?;
                if exhausted && job.method == "title_generation"
                    && let Some(conversation_id) = job.arguments["conversation_id"].as_i64()
                {
                    let hash = &uuid::Uuid::new_v4().to_string()[..8];
                    let fallback_title = format!("conv_{}", hash);
                    self.db.execute(
                        "UPDATE conversations SET title = ?1 WHERE id = ?2",
                        rusqlite::params![fallback_title, conversation_id]
                    )?;
                }
            }
        }

        Ok(())
    }

    fn mark_job_running(&self, job_id: i64) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        self.db.execute(
            "UPDATE background SET status = 'running', started_at = ?1 WHERE id = ?2",
            rusqlite::params![now, job_id]
        )?;
        Ok(())
    }

    fn mark_job_complete(&self, job_id: i64, result: &str) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        self.db.execute(
            "UPDATE background SET status = 'completed', completed_at = ?1, result = ?2 WHERE id = ?3",
            rusqlite::params![now, result, job_id]
        )?;
        Ok(())
    }

    fn mark_job_failed(&self, job_id: i64, error: &str) -> Result<bool> {
        let conn = self.db.lock()?;

        let (retries, max_retries): (i64, i64) = conn.query_row(
            "SELECT retries, max_retries FROM background WHERE id = ?1",
            rusqlite::params![job_id],
            |row| Ok((row.get(0)?, row.get(1)?))
        )?;

        let new_retries = retries + 1;
        let exhausted = new_retries >= max_retries;
        let status = if exhausted { "failed" } else { "pending" };

        conn.execute(
            "UPDATE background SET status = ?1, retries = ?2, result = ?3 WHERE id = ?4",
            rusqlite::params![status, new_retries, error, job_id]
        )?;

        Ok(exhausted)
    }
}
