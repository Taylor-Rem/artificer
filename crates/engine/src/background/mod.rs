use std::sync::Arc;
use anyhow::Result;
use tokio::time::{sleep, Duration};
use tokio::sync::watch;
use artificer_shared::rusqlite;

use crate::pool::{AgentPool, GpuPool};

#[derive(Debug)]
struct PendingJob {
    id: i64,
    #[allow(dead_code)]
    device_id: Option<i64>,
    method: String,
    arguments: serde_json::Value,
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

#[derive(Debug, serde::Serialize)]
pub struct WorkerHealth {
    pub pending_jobs: u64,
    pub running_jobs: u64,
    pub failed_jobs: u64,
    pub is_healthy: bool,
}

pub struct Worker {
    agent_pool: Arc<AgentPool>,
    gpu_pool: Arc<GpuPool>,
    poll_interval: Duration,
    shutdown_rx: watch::Receiver<bool>,
    last_cleanup: Arc<std::sync::Mutex<std::time::Instant>>,
}

impl Worker {
    pub fn new(
        agent_pool: Arc<AgentPool>,
        gpu_pool: Arc<GpuPool>,
        poll_interval_secs: u64,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            agent_pool,
            gpu_pool,
            poll_interval: Duration::from_secs(poll_interval_secs),
            shutdown_rx,
            last_cleanup: Arc::new(std::sync::Mutex::new(std::time::Instant::now())),
        }
    }

    pub async fn run(&self) -> Result<()> {
        println!("Background worker started");

        loop {
            if *self.shutdown_rx.borrow() {
                println!("Worker shutting down gracefully...");
                break;
            }

            if let Err(e) = self.process_next_job().await {
                eprintln!("Worker error: {}", e);
            }

            // Periodic cleanup (every 24 hours)
            {
                let mut last = self.last_cleanup.lock().unwrap();
                if last.elapsed().as_secs() > 86400 {
                    println!("Running background job cleanup...");
                    match self.agent_pool.db().cleanup_old_background_jobs() {
                        Ok(count) => println!("Cleaned up {} old background jobs", count),
                        Err(e) => eprintln!("Cleanup failed: {}", e),
                    }
                    *last = std::time::Instant::now();
                }
            }

            sleep(self.poll_interval).await;
        }

        Ok(())
    }

    pub async fn drain_queue(&self) -> Result<()> {
        println!("Processing remaining background jobs...");

        let mut processed = 0;
        let start_time = std::time::Instant::now();

        loop {
            let has_pending = self.has_pending_jobs()?;
            if !has_pending {
                break;
            }

            if let Err(e) = self.process_next_job().await {
                eprintln!("Error during drain: {}", e);
            } else {
                processed += 1;
                if processed % 5 == 0 {
                    println!("Processed {} background jobs...", processed);
                }
            }

            sleep(Duration::from_millis(100)).await;

            // Timeout after 30 seconds
            if start_time.elapsed().as_secs() > 30 {
                let remaining = self.agent_pool.db().lock()
                    .ok()
                    .and_then(|conn| {
                        conn.query_row(
                            "SELECT COUNT(*) FROM background WHERE status IN ('pending', 'running')",
                            [],
                            |row| row.get::<_, i64>(0)
                        ).ok()
                    })
                    .unwrap_or(0);

                println!("Background worker timeout - {} jobs remaining", remaining);
                break;
            }
        }

        println!("Processed {} background jobs in {:?}", processed, start_time.elapsed());
        Ok(())
    }

    /// Get a snapshot of the worker's current job queue health.
    pub fn health_status(&self) -> WorkerHealth {
        let db = self.agent_pool.db();
        let conn = db.lock().unwrap_or_else(|e| {
            eprintln!("DB lock failed in health_status: {}", e);
            panic!("DB lock poisoned");
        });

        let pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM background WHERE status = 'pending'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        let running: i64 = conn.query_row(
            "SELECT COUNT(*) FROM background WHERE status = 'running'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        let failed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM background WHERE status = 'failed'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        WorkerHealth {
            pending_jobs: pending as u64,
            running_jobs: running as u64,
            failed_jobs: failed as u64,
            is_healthy: running < 10,
        }
    }

    fn has_pending_jobs(&self) -> Result<bool> {
        let conn = self.agent_pool.db().lock()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM background WHERE status IN ('pending', 'running')",
            [],
            |row| row.get(0)
        )?;
        Ok(count > 0)
    }

    async fn process_next_job(&self) -> Result<()> {
        let job = self.agent_pool.db().query_row_optional(
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

        let gpu = match self.gpu_pool.acquire_background() {
            Some(gpu) => gpu,
            None => return Ok(()),
        };
        let gpu_id = gpu.id.clone();

        self.mark_job_running(job.id)?;

        let result = match job.method.as_str() {
            "title_generation" => {
                let agent = match self.agent_pool.get("TitleGenerator") {
                    Some(a) => a,
                    None => {
                        self.gpu_pool.release(&gpu_id);
                        return Err(anyhow::anyhow!("TitleGenerator agent not found"));
                    }
                };

                let conversation_id = job.arguments["conversation_id"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("Missing conversation_id in job args"))?;
                let user_message = job.arguments["user_message"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing user_message in job args"))?
                    .to_string();

                let context = crate::agent::state::ExecutionContext {
                    device_id: job.device_id.unwrap_or(0) as u64,
                    device_key: String::new(),
                    conversation_id,
                    parent_task_id: None,
                    gpu: gpu.clone(),
                    events: None,
                    db: self.agent_pool.db().clone(),
                };

                let execution = crate::agent::AgentExecution::new(
                    agent,
                    context,
                    &user_message,
                    &self.agent_pool,
                );

                let response = execution.execute(self.agent_pool.clone()).await?;

                let device_id = job.device_id.unwrap_or(0);
                self.agent_pool
                    .db()
                    .set_conversation_title(conversation_id, device_id, &response.content)?;

                Ok(format!("Set title: {}", response.content))
            }
            other => Err(anyhow::anyhow!("Unknown job method: {}", other)),
        };

        self.gpu_pool.release(&gpu_id);

        match result {
            Ok(res) => self.mark_job_complete(job.id, &res)?,
            Err(e) => {
                let _ = self.mark_job_failed(job.id, &e.to_string())?;
            }
        }

        Ok(())
    }

    fn mark_job_running(&self, job_id: i64) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        self.agent_pool.db().execute(
            "UPDATE background SET status = 'running', started_at = ?1 WHERE id = ?2",
            rusqlite::params![now, job_id]
        )?;
        Ok(())
    }

    fn mark_job_complete(&self, job_id: i64, result: &str) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        self.agent_pool.db().execute(
            "UPDATE background SET status = 'completed', completed_at = ?1, result = ?2 WHERE id = ?3",
            rusqlite::params![now, result, job_id]
        )?;
        Ok(())
    }

    fn mark_job_failed(&self, job_id: i64, error: &str) -> Result<bool> {
        let conn = self.agent_pool.db().lock()?;

        let (retries, max_retries): (i64, i64) = conn.query_row(
            "SELECT retries, max_retries FROM background WHERE id = ?1",
            rusqlite::params![job_id],
            |row| Ok((row.get(0)?, row.get(1)?))
        )?;

        let new_retries = retries + 1;
        let exhausted = new_retries >= max_retries;
        let status = if exhausted { "failed" } else { "pending" };

        let error_msg = format!(
            "Attempt {}/{} failed: {}",
            new_retries,
            max_retries,
            error
        );

        conn.execute(
            "UPDATE background SET status = ?1, retries = ?2, result = ?3 WHERE id = ?4",
            rusqlite::params![status, new_retries, error_msg, job_id]
        )?;

        // When exhausted, apply job-specific fallback behavior
        if exhausted {
            let method: String = conn.query_row(
                "SELECT method FROM background WHERE id = ?1",
                rusqlite::params![job_id],
                |row| row.get(0),
            )?;

            if method == "title_generation" {
                let args_str: String = conn.query_row(
                    "SELECT arguments FROM background WHERE id = ?1",
                    rusqlite::params![job_id],
                    |row| row.get(0),
                )?;

                if let Ok(args) = serde_json::from_str::<serde_json::Value>(&args_str) {
                    if let Some(conv_id) = args["conversation_id"].as_i64() {
                        let hash = &uuid::Uuid::new_v4().to_string()[..8];
                        let fallback = format!("conversation_{}", hash);
                        let _ = conn.execute(
                            "UPDATE conversations SET title = ?1 WHERE id = ?2 AND title IS NULL",
                            rusqlite::params![fallback, conv_id],
                        );
                    }
                }
            }
        }

        Ok(exhausted)
    }
}
