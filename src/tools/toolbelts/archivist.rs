use crate::register_toolbelt;
use crate::memory::Db;
use anyhow::Result;
use crate::services::title::sanitize_title;

#[derive(Clone)]
pub struct Archivist {
    db: Db,
}

impl Default for Archivist {
    fn default() -> Self {
        Self {
            db: Db::default(),
        }
    }
}

register_toolbelt! {
    Archivist {
        description: "Tool for managing chat history, user memory, and preferences",
        tools: {
            "query_db" => query_db {
                description: "Runs a SQL query against the database and returns results as JSON",
                params: ["query": "string" => "SQL query string", "params": "array" => "Ordered parameter values for ?1, ?2, etc."]
            },
            "list_tables" => list_tables {
                description: "Lists all tables in the database",
                params: []
            },
            "list_conversations" => list_conversations {
                description: "Lists all conversations with their IDs and titles",
                params: []
            },
            "get_summary" => get_summary {
                description: "Get the conversation summary by title",
                params: ["title": "string" => "Title of the conversation to retrieve"]
            },
            "get_conversation" => get_conversation {
                description: "Retrieves a conversation and all messages by its title",
                params: ["title": "string" => "Title of the conversation to retrieve"]
            },
        }
    }
}

impl Archivist {
    fn query_db(&self, args: &serde_json::Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return Ok("Error: query cannot be empty".to_string());
        }
        let params: Vec<rusqlite::types::Value> = args["params"]
            .as_array()
            .map(|arr| arr.iter().map(Self::json_to_rusqlite).collect())
            .unwrap_or_default();
        self.db.query(query, rusqlite::params_from_iter(params))
    }

    fn list_tables(&self, _args: &serde_json::Value) -> Result<String> {
        self.db.query(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
            [],
        )
    }

    fn list_conversations(&self, _args: &serde_json::Value) -> Result<String> {
        self.db.query(
            "SELECT id, title, created, last_accessed FROM conversations ORDER BY last_accessed DESC",
            []
        )
    }

    fn get_summary(&self, args: &serde_json::Value) -> Result<String> {
        let title = sanitize_title(args["title"].as_str().unwrap_or(""));
        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }
        self.db.query(
            "SELECT summary FROM conversations WHERE title = ?1",
            rusqlite::params![title],
        )
    }

    fn get_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("");

        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }

        let conn = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        // First get conversation metadata
        let conv_result = conn.query_row(
            "SELECT id, title FROM conversations WHERE title = ?1",
            rusqlite::params![title],
            |row| {
                let id: i64 = row.get(0)?;
                let title: Option<String> = row.get(1)?;
                Ok((id, title))
            },
        );

        match conv_result {
            Ok((conv_id, conv_title)) => {
                let mut output = String::new();

                // Add conversation header
                output.push_str(&format!("title: {}\n", conv_title.unwrap_or("Untitled".to_string())));

                // Get tasks used in this conversation
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT t.title
                     FROM task_history th
                     JOIN tasks t ON th.task_id = t.id
                     WHERE th.conversation_id = ?1
                     ORDER BY th.created"
                )?;

                let tasks: Vec<String> = stmt.query_map(rusqlite::params![conv_id], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();

                if !tasks.is_empty() {
                    output.push_str(&format!("tasks_used: {}\n", tasks.join(", ")));
                }

                output.push_str("\nmessages:\n");

                // Get all messages for this conversation
                let mut stmt = conn.prepare(
                    "SELECT role, message FROM messages
                     WHERE conversation_id = ?1
                     ORDER BY m_order"
                )?;

                let messages = stmt.query_map(rusqlite::params![conv_id], |row| {
                    let role: String = row.get(0)?;
                    let message: String = row.get(1)?;
                    Ok((role, message))
                })?;

                for message in messages {
                    if let Ok((role, content)) = message {
                        output.push_str(&format!("\nrole: {}\n", role));
                        output.push_str(&format!("message: {}\n", content));
                    }
                }

                Ok(output)
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Ok(format!("Error: Conversation '{}' not found", title))
            }
            Err(e) => Ok(format!("Error retrieving conversation: {}", e)),
        }
    }

    fn json_to_rusqlite(val: &serde_json::Value) -> rusqlite::types::Value {
        match val {
            serde_json::Value::Null => rusqlite::types::Value::Null,
            serde_json::Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    rusqlite::types::Value::Integer(i)
                } else {
                    rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => rusqlite::types::Value::Text(s.clone()),
            other => rusqlite::types::Value::Text(other.to_string()),
        }
    }
}
