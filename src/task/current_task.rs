use anyhow::Result;
use crate::Message;
use crate::memory::Db;
use crate::task::Task;

pub struct CurrentTask {
    db: Db
}

impl Default for CurrentTask {
    fn default() -> Self {
        Self {
            db: Db::default()
        }
    }
}

impl CurrentTask {
    pub async fn init(&self, user_message: Message, location: &str, current_task: &Task) -> Result<u64> {
        let th_id = self.create_task_history_entry(location.to_string(), current_task).await?;

        let _ = self.create_title(th_id, &user_message);
        Ok(th_id)
    }

    pub async fn create_task_history_entry(&self, location: String, current_task: &Task) -> Result<u64> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let task_id = current_task.task_id();

        conn.execute(
            "INSERT INTO task_history (task_id, location, created, last_accessed) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![task_id, location, now, now],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    pub fn create_message(&self, th_id: Option<u64>, role: &str, message: &str, message_count: &mut u32) -> Result<()> {
        let th_id = th_id.ok_or_else(|| anyhow::anyhow!("No task_history ID"))?;

        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO messages (task_history_id, role, message, m_order, created) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![th_id as i64, role, message, *message_count as i64, now],
        )?;

        *message_count += 1;
        Ok(())
    }


    fn create_title(&self, th_id: u64, user_message: &Message) -> Result<u64> {
        self.db.create_job(
            Task::TitleGeneration,
            &serde_json::json!({
            "th_id": th_id,
            "user_message": {
                "role": &user_message.role,
                "content": &user_message.content,
            }
        }),
            1
        )
    }

    pub fn summarize(&self, th_id: u64) -> Result<u64> {
        self.db.create_job(Task::Summarization, &serde_json::json!({ "th_id": th_id }), 0)
    }

    pub fn extract_memory(&self, th_id: u64) -> Result<u64> {
        self.db.create_job(Task::MemoryExtraction, &serde_json::json!({ "th_id": th_id }), 0)
    }
}
