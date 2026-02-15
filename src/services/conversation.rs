use anyhow::Result;
use crate::Message;
use crate::memory::Db;
use crate::task::Task;

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
            "INSERT INTO conversations (location, created, last_accessed) VALUES (?1, ?2, ?3)",
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
            "INSERT INTO messages (conversation_id, role, message, \"order\", created) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![conv_id as i64, role, message, *message_count as i64, now],
        )?;

        *message_count += 1;
        Ok(())
    }


    fn create_title(&self, conversation_id: u64, user_message: &Message) -> Result<u64> {
        self.db.create_job(
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

    pub fn summarize(&self, conversation_id: u64) -> Result<u64> {
        self.db.create_job(Task::Summarization, &serde_json::json!({ "conversation_id": conversation_id }), 0)
    }
}
