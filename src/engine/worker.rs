use anyhow::Result;
use tokio::time::{sleep, Duration};
use serde_json::Value;
use crate::engine::db::Db;
use crate::agents::helper::Helper;
use crate::services::title::Title;
use crate::Message;

pub struct Worker {
    db: Db,
    helper: Helper,
    title_service: Title,
    poll_interval: Duration,
}

impl Worker {
    pub fn new(poll_interval_secs: u64) -> Self {
        Self {
            db: Db::default(),
            helper: Helper,
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
        // Get highest priority pending job
        let job_json = self.db.query(
            "SELECT id, method, arguments, context FROM jobs
             WHERE status = 'pending'
             ORDER BY priority DESC, created_at ASC
             LIMIT 1",
            []
        )?;

        let jobs: Vec<Value> = serde_json::from_str(&job_json)?;
        if jobs.is_empty() {
            return Ok(());
        }

        let job = &jobs[0];
        let job_id = job["id"].as_i64().unwrap();
        let method = job["method"].as_str().unwrap();
        let arguments: Value = serde_json::from_str(job["arguments"].as_str().unwrap())?;
        let context: Option<Value> = job["context"]
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok());

        // Mark as running
        self.mark_job_running(job_id)?;

        // Execute based on method
        let result = match method {
            "create_title" => self.create_title(&arguments, &context).await,
            "create_summary" => self.create_summary(&arguments, &context).await,
            _ => Err(anyhow::anyhow!("Unknown method: {}", method)),
        };

        // Update job status
        match result {
            Ok(res) => self.mark_job_complete(job_id, &res)?,
            Err(e) => {
                let exhausted = self.mark_job_failed(job_id, &e.to_string())?;
                if exhausted && method == "create_title" {
                    // Fallback: assign a random hash title
                    if let Some(conversation_id) = arguments["conversation_id"].as_i64() {
                        let hash = &uuid::Uuid::new_v4().to_string()[..8];
                        let fallback_title = format!("conv_{}", hash);
                        self.db.execute(
                            "UPDATE conversation SET title = ?1 WHERE id = ?2",
                            rusqlite::params![fallback_title, conversation_id]
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn create_title(&self, args: &Value, context: &Option<Value>) -> Result<String> {
        let conversation_id = args["conversation_id"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

        let user_message = context
            .as_ref()
            .and_then(|c| c.get("user_message"))
            .ok_or_else(|| anyhow::anyhow!("Missing user_message in context"))?;

        let message = Message {
            role: user_message["role"].as_str().unwrap_or("user").to_string(),
            content: user_message["content"].as_str().map(String::from),
            tool_calls: None,
        };

        let raw_title = self.helper.create_title(&message).await?;
        let sanitized = self.title_service.sanitize_title(&raw_title);

        if sanitized.is_empty() {
            return Err(anyhow::anyhow!("Generated title was empty after sanitization"));
        }

        let final_title = if self.title_service.title_exists(&sanitized) {
            self.title_service.find_available_title(&sanitized)
        } else {
            sanitized
        };

        // Update conversation with validated title
        self.db.execute(
            "UPDATE conversation SET title = ?1 WHERE id = ?2",
            rusqlite::params![final_title, conversation_id]
        )?;

        Ok(final_title)
    }

    async fn create_summary(&self, args: &Value, _context: &Option<Value>) -> Result<String> {
        let conversation_id = args["conversation_id"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing conversation_id"))?;

        // Fetch conversation messages
        let messages_json = self.db.query(
            "SELECT role, message FROM message
             WHERE conversation_id = ?1
             ORDER BY \"order\"",
            rusqlite::params![conversation_id]
        )?;

        let messages: Vec<Value> = serde_json::from_str(&messages_json)?;
        let text: String = messages.iter()
            .map(|m| format!("{}: {}", m["role"].as_str().unwrap_or(""), m["message"].as_str().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n");

        let summary = self.helper.summarize(&text).await?;

        self.db.execute(
            "UPDATE conversation SET summary = ?1 WHERE id = ?2",
            rusqlite::params![summary, conversation_id]
        )?;

        Ok(summary)
    }

    fn mark_job_running(&self, job_id: i64) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        self.db.execute(
            "UPDATE jobs SET status = 'running', started_at = ?1 WHERE id = ?2",
            rusqlite::params![now, job_id]
        )?;
        Ok(())
    }

    fn mark_job_complete(&self, job_id: i64, result: &str) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        self.db.execute(
            "UPDATE jobs SET status = 'completed', completed_at = ?1, result = ?2 WHERE id = ?3",
            rusqlite::params![now, result, job_id]
        )?;
        Ok(())
    }

    /// Returns `true` if retries are exhausted (job marked as "failed").
    fn mark_job_failed(&self, job_id: i64, error: &str) -> Result<bool> {
        let conn = self.db.lock()?;

        // Get current retry count
        let retries: i64 = conn.query_row(
            "SELECT retries FROM jobs WHERE id = ?1",
            rusqlite::params![job_id],
            |row| row.get(0)
        )?;

        let max_retries: i64 = conn.query_row(
            "SELECT max_retries FROM jobs WHERE id = ?1",
            rusqlite::params![job_id],
            |row| row.get(0)
        )?;

        let new_retries = retries + 1;
        let exhausted = new_retries >= max_retries;
        let status = if exhausted { "failed" } else { "pending" };

        conn.execute(
            "UPDATE jobs SET status = ?1, retries = ?2, result = ?3 WHERE id = ?4",
            rusqlite::params![status, new_retries, error, job_id]
        )?;

        Ok(exhausted)
    }
}
