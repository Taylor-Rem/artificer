use anyhow::Result;
use crate::Message;
use crate::memory::Db;
use crate::task::Task;

pub struct Conversation {
    db: Db,
    device_id: i64,
}

impl Conversation {
    pub fn new(device_id: i64) -> Self {
        Self {
            db: Db::default(),
            device_id,
        }
    }
}

impl Conversation {
    /// Initialize a new conversation or continue an existing one
    pub async fn init(&self, user_message: &Message, current_task: &Task) -> Result<(u64, u64)> {
        // Create conversation for this device
        let conversation_id = self.create_conversation_entry().await?;

        // Create task history entry for this task execution
        let task_history_id = self.create_task_history_entry(conversation_id, current_task).await?;

        // Queue title generation
        let _ = self.create_title_job(conversation_id, user_message);

        Ok((conversation_id, task_history_id))
    }

    /// Create a new conversation entry for this device
    async fn create_conversation_entry(&self) -> Result<u64> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO conversations (device_id, created, last_accessed) VALUES (?1, ?2, ?3)",
            rusqlite::params![self.device_id, now, now],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    /// Create a task history entry for a task execution within a conversation
    async fn create_task_history_entry(&self, conversation_id: u64, current_task: &Task) -> Result<u64> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let task_id = current_task.task_id();

        conn.execute(
            "INSERT INTO task_history (device_id, task_id, conversation_id, location, created, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'running')",
            rusqlite::params![self.device_id, task_id, conversation_id as i64, "", now],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    /// Add a message to the conversation
    pub fn add_message(&self, conversation_id: Option<u64>, role: &str, message: &str, message_count: &mut u32) -> Result<()> {
        let conversation_id = conversation_id.ok_or_else(|| anyhow::anyhow!("No conversation ID"))?;

        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO messages (conversation_id, role, message, m_order, created)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![conversation_id as i64, role, message, *message_count as i64, now],
        )?;

        *message_count += 1;

        // Update last_accessed time
        conn.execute(
            "UPDATE conversations SET last_accessed = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id as i64],
        )?;

        Ok(())
    }

    /// Mark a task execution as completed
    pub fn complete_task(&self, task_history_id: u64) -> Result<()> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "UPDATE task_history SET status = 'completed', completed = ?1 WHERE id = ?2",
            rusqlite::params![now, task_history_id as i64],
        )?;

        Ok(())
    }

    /// Mark a task execution as failed
    pub fn fail_task(&self, task_history_id: u64) -> Result<()> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "UPDATE task_history SET status = 'failed', completed = ?1 WHERE id = ?2",
            rusqlite::params![now, task_history_id as i64],
        )?;

        Ok(())
    }

    /// Start a new task execution within the same conversation
    pub async fn switch_task(&self, conversation_id: u64, new_task: &Task) -> Result<u64> {
        self.create_task_history_entry(conversation_id, new_task).await
    }

    /// Queue title generation job
    fn create_title_job(&self, conversation_id: u64, user_message: &Message) -> Result<u64> {
        self.db.create_job(
            self.device_id,
            Task::TitleGeneration,
            &serde_json::json!({
                "conversation_id": conversation_id,
                "user_message": {
                    "role": &user_message.role,
                    "content": &user_message.content,
                }
            }),
            1
        )
    }

    /// Queue summarization job
    pub fn summarize(&self, conversation_id: u64) -> Result<u64> {
        self.db.create_job(
            self.device_id,
            Task::Summarization,
            &serde_json::json!({ "conversation_id": conversation_id }),
            0
        )
    }

    /// Queue memory extraction job
    pub fn extract_memory(&self, conversation_id: u64) -> Result<u64> {
        self.db.create_job(
            self.device_id,
            Task::MemoryExtraction,
            &serde_json::json!({ "conversation_id": conversation_id }),
            0
        )
    }

    /// Get all messages for a conversation
    pub fn get_messages(&self, conversation_id: u64) -> Result<Vec<Message>> {
        let conn = self.db.lock()?;
        let mut stmt = conn.prepare(
            "SELECT role, message FROM messages
             WHERE conversation_id = ?1
             ORDER BY m_order"
        )?;

        let messages = stmt.query_map(rusqlite::params![conversation_id as i64], |row| {
            Ok(Message {
                role: row.get(0)?,
                content: Some(row.get(1)?),
                tool_calls: None,
            })
        })?;

        messages.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// Get conversation title
    pub fn get_title(&self, conversation_id: u64) -> Result<Option<String>> {
        self.db.query_row_optional(
            "SELECT title FROM conversations WHERE id = ?1",
            rusqlite::params![conversation_id as i64],
            |row| row.get(0)
        )
    }

    /// Update conversation last accessed time
    pub fn touch(&self, conversation_id: u64) -> Result<()> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "UPDATE conversations SET last_accessed = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id as i64],
        )?;

        Ok(())
    }
}