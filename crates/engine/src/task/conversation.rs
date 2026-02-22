use anyhow::Result;
use crate::Message;
use artificer_shared::{db::Db, rusqlite};
use crate::task::{Task, specialist::ToolCall};

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
    pub async fn init(&self, user_message: &Message) -> Result<u64> {
        let conversation_id = self.create_conversation_entry().await?;
        let _ = self.create_title_job(conversation_id, user_message);
        Ok(conversation_id)
    }

    async fn create_conversation_entry(&self) -> Result<u64> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Check if device exists
        let device_exists: bool = conn.query_row(
            "SELECT 1 FROM devices WHERE id = ?1",
            rusqlite::params![self.device_id],
            |_| Ok(true)
        ).unwrap_or(false);

        if !device_exists {
            return Err(anyhow::anyhow!(
            "Device {} does not exist. Device must be registered before creating conversations.",
            self.device_id
        ));
        }

        conn.execute(
            "INSERT INTO conversations (device_id, created, last_accessed) VALUES (?1, ?2, ?3)",
            rusqlite::params![self.device_id, now, now],
        ).map_err(|e| {
            anyhow::anyhow!("Failed to create conversation for device {}: {}", self.device_id, e)
        })?;

        Ok(conn.last_insert_rowid() as u64)
    }

    pub fn add_message(
        &self,
        conversation_id: Option<u64>,
        role: &str,
        message: Option<&str>,
        tool_calls: Option<&Vec<ToolCall>>,
        message_count: &mut u32,
    ) -> Result<()> {
        let tool_calls_json = tool_calls
            .map(|tc| serde_json::to_string(tc))
            .transpose()?;

        let conversation_id = conversation_id.ok_or_else(|| anyhow::anyhow!("No conversation ID"))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let conn = self.db.lock()?;
        conn.execute(
            "INSERT INTO messages (conversation_id, role, message, tool_calls, m_order, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                conversation_id as i64, role, message,
                tool_calls_json, *message_count as i64, now
            ],
        )?;
        *message_count += 1;
        conn.execute(
            "UPDATE conversations SET last_accessed = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id as i64],
        )?;
        Ok(())
    }

    /// Queue title generation job
    fn create_title_job(&self, conversation_id: u64, user_message: &Message) -> Result<u64> {
        self.db.create_job(
            self.device_id,
            Task::TitleGeneration.title(),
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
            Task::Summarization.title(),
            &serde_json::json!({ "conversation_id": conversation_id }),
            0
        )
    }

    /// Queue memory extraction job
    pub fn extract_memory(&self, conversation_id: u64) -> Result<u64> {
        self.db.create_job(
            self.device_id,
            Task::MemoryExtraction.title(),
            &serde_json::json!({ "conversation_id": conversation_id }),
            0
        )
    }

    pub fn get_messages(&self, conversation_id: u64) -> Result<Vec<Message>> {
        let conn = self.db.lock()?;
        let mut stmt = conn.prepare(
            "SELECT role, message, tool_calls FROM messages
             WHERE conversation_id = ?1
             ORDER BY m_order"
        )?;

        let messages = stmt.query_map(rusqlite::params![conversation_id as i64], |row| {
            let role: String = row.get(0)?;
            let message: Option<String> = row.get(1)?;
            let tool_calls_json: Option<String> = row.get(2)?;
            Ok((role, message, tool_calls_json))
        })?
            .filter_map(|r| r.ok())
            .map(|(role, message, tool_calls_json)| {
                let tool_calls = tool_calls_json
                    .and_then(|j| serde_json::from_str(&j).ok());
                Message { role, content: message, tool_calls }
            })
            .collect();

        Ok(messages)
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