use anyhow::Result;
use crate::Message;
use crate::engine::db::Db;

pub struct Conversation {
    db: Db
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            db: Db::default()
        }
    }
}

impl Conversation {
    pub async fn init(&self, user_message: Message, location: &str) -> Result<u64> {
        let conversation_id = self.create_conversation(location.to_string())?;
        let _ = self.create_title(conversation_id, &user_message);
        Ok(conversation_id)
    }

    pub fn create_conversation(&self, location: String) -> Result<u64> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO conversation (location, created, last_accessed) VALUES (?1, ?2, ?3)",
            rusqlite::params![location, now, now],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    pub fn create_message(&self, conversation_id: Option<u64>, role: &str, message: &str, message_count: &mut u32) -> Result<()> {
        let conv_id = conversation_id.ok_or_else(|| anyhow::anyhow!("No conversation ID"))?;

        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO message (conversation_id, role, message, \"order\", created) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![conv_id as i64, role, message, *message_count as i64, now],
        )?;

        *message_count += 1;
        Ok(())
    }

    pub fn create_job(&self, method: &str, arguments: &serde_json::Value, context: Option<&serde_json::Value>, priority: i32) -> Result<u64> {
        let conn = self.db.lock()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO jobs (method, arguments, priority, status, created_at, context)
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5)",
            rusqlite::params![
                method,
                arguments.to_string(),
                priority,
                now,
                context.map(|c| c.to_string())
            ],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    fn create_title(&self, conversation_id: u64, user_message: &Message) -> Result<u64> {
        let context = serde_json::json!({
            "user_message": {
                "role": user_message.role,
                "content": user_message.content,
            }
        });
        self.create_job("create_title", &serde_json::json!({ "conversation_id": conversation_id }), Some(&context), 1)
    }

    pub fn summarize(&self, conversation_id: u64) -> Result<u64> {
        self.create_job("create_summary", &serde_json::json!({ "conversation_id": conversation_id }), None, 0)
    }
}
