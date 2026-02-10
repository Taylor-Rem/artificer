use shared::register_toolbelt;
use shared::traits::{ParameterSchema, ToolSchema};
use shared::Message;
use helper::Helper;
use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;
use std::sync::{Arc, Mutex};
#[derive(Clone)]
pub struct Archivist {
    db: Arc<Mutex<Connection>>,
}

impl Default for Archivist {
    fn default() -> Self {
        let db_path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("RustroverProjects")
            .join("specialists")
            .join("memory.db");

        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let conn = Connection::open(&db_path).expect("Failed to open database");

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversation (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT,
                location TEXT NOT NULL,
                created INTEGER NOT NULL,
                last_accessed INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_title ON conversation(title);

            CREATE TABLE IF NOT EXISTS message (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER,
                role TEXT NOT NULL,
                message TEXT NOT NULL,
                \"order\" INTEGER NOT NULL,
                created INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_conversation_id ON message(conversation_id);"
        ).expect("Failed to create tables");

        Self {
            db: Arc::new(Mutex::new(conn)),
        }
    }
}

register_toolbelt! {
    Archivist {
        description: "Tool for managing chat history, user memory, and preferences",
        tools: {
            "list_conversations" => list_conversations {
                description: "Lists all conversations with their IDs, titles, and locations",
                params: []
            },
            "retrieve_conversation" => retrieve_conversation {
                description: "Retrieves a conversation by its title",
                params: ["title": "string" => "Title of the conversation to retrieve"]
            },
        }
    }
}

impl Archivist {
     pub async fn initialize_conversation(&self, user_message: Message, location: &str) -> Result<u64> {
        let conversation_id = self.create_conversation(location.to_string())?;
         let archivist_clone = self.clone();
         tokio::spawn(async move {
             archivist_clone.create_title(conversation_id, user_message).await;
         });
         Ok(conversation_id)
    }

    async fn create_title(&self, conversation_id: u64, user_message: Message) {
        let helper = Helper;
        let mut attempts = 0;
        let max_attempts = 3;
        let mut failed_titles: Vec<String> = Vec::new();
        let mut all_null = true;

        while attempts < max_attempts {
            attempts += 1;

            match helper.create_title(&user_message).await {
                Ok(raw_title) if !raw_title.is_empty() => {
                    all_null = false;

                    let sanitized = self.sanitize_title(&raw_title);

                    if !self.title_exists(&sanitized) {
                        if let Ok(conn) = self.db.lock() {
                            let _ = conn.execute(
                                "UPDATE conversation SET title = ?1 WHERE id = ?2",
                                rusqlite::params![sanitized, conversation_id as i64],
                            );
                        }
                        return;
                    }

                    failed_titles.push(sanitized);
                }
                _ => {}
            }
        }

        let final_title = if all_null {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            conversation_id.hash(&mut hasher);
            std::time::SystemTime::now().hash(&mut hasher);
            format!("conversation_{:x}", hasher.finish())
        } else {
            let base_title = failed_titles.first().unwrap();
            self.find_available_title(base_title)
        };

        if let Ok(conn) = self.db.lock() {
            let _ = conn.execute(
                "UPDATE conversation SET title = ?1 WHERE id = ?2",
                rusqlite::params![final_title, conversation_id as i64],
            );
        }
    }

    fn sanitize_title(&self, title: &str) -> String {
        title.chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' => c,
                ' ' | '-' | '.' | '/' | '\\' => '_',
                _ => '_',
            })
            .collect::<String>()
            .split('_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_")
    }

    fn title_exists(&self, title: &str) -> bool {
        if let Ok(conn) = self.db.lock() {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM conversation WHERE title = ?1 LIMIT 1",
                    rusqlite::params![title],
                    |_row| Ok(true),
                )
                .unwrap_or(false);
            return exists;
        }
        false
    }

    fn find_available_title(&self, base: &str) -> String {
        let mut counter = 1;
        loop {
            let candidate = format!("{}_{}", base, counter);
            if !self.title_exists(&candidate) {
                return candidate;
            }
            counter += 1;

            if counter > 1000 {
                return format!("{}_{}", base, uuid::Uuid::new_v4().to_string());
            }
        }
    }

    pub fn create_conversation(&self, location: String) -> Result<u64> {
        let conn = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO conversation (title, location, created, last_accessed) VALUES (NULL, ?1, ?2, ?3)",
            rusqlite::params![location, now, now],
        )?;

        Ok(conn.last_insert_rowid() as u64)
    }

    pub fn create_message(&self, conversation_id: Option<u64>, role: &str, message: &str, message_count: &mut u32) -> Result<()> {
        let conv_id = conversation_id.ok_or_else(|| anyhow::anyhow!("No conversation ID"))?;

        let conn = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
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

    fn list_conversations(&self, _args: &serde_json::Value) -> Result<String> {
        let conn = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let mut stmt = conn.prepare("SELECT id, title, location FROM conversation")?;
        let conversations: Vec<serde_json::Value> = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let title: Option<String> = row.get(1)?;
                let location: String = row.get(2)?;
                Ok(json!({
                    "conversation_id": id,
                    "title": title,
                    "location": location
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let count = conversations.len();
        Ok(json!({
            "conversations": conversations,
            "count": count
        }).to_string())
    }

    fn retrieve_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("");

        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }

        let conn = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let conv_result = conn.query_row(
            "SELECT id, title, location FROM conversation WHERE title = ?1",
            rusqlite::params![title],
            |row| {
                let id: i64 = row.get(0)?;
                let title: Option<String> = row.get(1)?;
                let location: String = row.get(2)?;
                Ok((id, title, location))
            },
        );

        match conv_result {
            Ok((conv_id, conv_title, conv_location)) => {
                let mut stmt = conn.prepare(
                    "SELECT id, role, message FROM message WHERE conversation_id = ?1 ORDER BY \"order\""
                )?;
                let messages: Vec<serde_json::Value> = stmt
                    .query_map(rusqlite::params![conv_id], |row| {
                        let msg_id: i64 = row.get(0)?;
                        let role: String = row.get(1)?;
                        let message: String = row.get(2)?;
                        Ok(json!({
                            "message_id": msg_id,
                            "role": role,
                            "message": message,
                        }))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();

                Ok(json!({
                    "conversation_id": conv_id,
                    "title": conv_title,
                    "location": conv_location,
                    "messages": messages,
                }).to_string())
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Ok(json!({
                    "error": "Conversation not found",
                    "title": title
                }).to_string())
            }
            Err(e) => Ok(format!("Error retrieving conversation: {}", e)),
        }
    }
}
