use crate::register_toolbelt;
use crate::schema::{ParameterSchema, ToolSchema};
use crate::core::db::Db;
use anyhow::Result;

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
        self.db.query("SELECT id, title, location FROM conversation", [])
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

    fn retrieve_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("");
        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }
        self.db.query(
            "SELECT c.id, c.title, c.location, m.id as message_id, m.role, m.message \
             FROM conversation c \
             LEFT JOIN message m ON m.conversation_id = c.id \
             WHERE c.title = ?1 \
             ORDER BY m.\"order\"",
            rusqlite::params![title],
        )
    }
}
