use anyhow::Result;
use tokio::time::{sleep, Duration};
use serde_json::Value;
use crate::memory::Db;
use crate::services::title::Title;
use crate::task::background::{summarization, title_generation};
use crate::task::specialist::Specialist;
use crate::task::Task;

pub struct JobContext<'a> {
    pub db: &'a Db,
    pub specialist: &'a Specialist,
    pub title_service: &'a Title,
}

pub async fn execute(task: &Task, ctx: &JobContext<'_>, args: &Value) -> anyhow::Result<String> {
    match task {
        Task::TitleGeneration => title_generation::execute(ctx, args).await,
        Task::Summarization => summarization::execute(ctx, args).await,
        _ => Err(anyhow::anyhow!("Task not implemented: {:?}", task)),
    }
}

#[derive(Debug)]
struct PendingJob {
    id: i64,
    task: super::Task,
    arguments: Value,
}

impl PendingJob {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let id = row.get(0)?;
        let method_str: String = row.get(1)?;
        let arguments_str: String = row.get(2)?;

        let task = super::Task::from_str(&method_str)
            .ok_or_else(|| rusqlite::Error::InvalidQuery)?;
        let arguments = serde_json::from_str(&arguments_str)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;

        Ok(PendingJob { id, task, arguments })
    }
}

pub struct Worker {
    db: Db,
    title_service: Title,
    poll_interval: Duration,
}

impl Worker {
    pub fn new(poll_interval_secs: u64) -> Self {
        Self {
            db: Db::default(),
            title_service: Title::default(),
            poll_interval: Duration::from_secs(poll_interval_secs),
        }
    }

    pub async fn run(&self) -> Result<()> {
        loop {
            if let Err(e) = self.process_next_job().await {
                eprintln!("Worker error: {}", e);
            }
            sleep(self.poll_interval).await;
        }
    }

    async fn process_next_job(&self) -> Result<()> {
        let job = self.db.query_row_optional(
            "SELECT id, method, arguments FROM background
             WHERE status = 'pending'
             ORDER BY priority DESC, created_at ASC
             LIMIT 1",
            [],
            PendingJob::from_row
        )?;

        let Some(job) = job else {
            return Ok(());
        };

        self.mark_job_running(job.id)?;

        let specialist = job.task.specialist();
        let ctx = JobContext {
            db: &self.db,
            specialist: &specialist,
            title_service: &self.title_service,
        };

        let result = execute(&job.task, &ctx, &job.arguments).await;

        match result {
            Ok(res) => self.mark_job_complete(job.id, &res)?,
            Err(e) => {
                let exhausted = self.mark_job_failed(job.id, &e.to_string())?;
                if exhausted && matches!(job.task, super::Task::TitleGeneration) {
                    if let Some(th_id) = job.arguments["th_id"].as_i64() {
                        let hash = &uuid::Uuid::new_v4().to_string()[..8];
                        let fallback_title = format!("conv_{}", hash);
                        self.db.execute(
                            "UPDATE conversations SET title = ?1 WHERE id = ?2",
                            rusqlite::params![fallback_title, th_id]
                        )?;
                    }
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

    /// Returns `true` if retries are exhausted (job marked as "failed").
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
