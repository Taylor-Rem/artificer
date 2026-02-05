use crate::register_toolbelt;
use crate::traits::{ParameterSchema, ToolSchema};
use anyhow::Result;
use db::{Database, DataType, Value, Row};
use db::TableBuilder;
use serde_json::json;
use std::sync::Mutex;

pub struct Archivist {
    db: Mutex<Database>,
}

impl Default for Archivist {
    fn default() -> Self {
        let db_path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".artificer")
            .join("memory.db");

        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut db = if db_path.exists() {
            Database::open(&db_path).expect("Failed to open database")
        } else {
            Database::create(&db_path).expect("Failed to create database")
        };

        // Create conversation table with title and location
        if db.get_schema("conversation").is_none() {
            let schema = TableBuilder::new("conversation")
                .column_not_null("conversation_id", DataType::Int64)
                .column_not_null("title", DataType::Text)
                .column("location", DataType::Text)
                .primary_key(&["conversation_id"])
                .build();

            db.create_table(schema).expect("Failed to create conversation table");
        }

        Self {
            db: Mutex::new(db),
        }
    }
}

register_toolbelt! {
    Archivist {
        description: "Tool for managing chat history, user memory, and preferences",
        tools: {
            "create_conversation" => create_conversation {
                description: "Creates a new conversation with a unique ID and title",
                params: [
                    "conversation_id": "integer" => "Unique conversation ID",
                    "title": "string" => "Conversation title",
                    "location": "string" => "Directory path where conversation started"
                ]
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
    fn value_to_json(value: &Value) -> serde_json::Value {
        match value {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => json!(b),
            Value::Int32(n) => json!(n),
            Value::Int64(n) => json!(n),
            Value::Float64(f) => json!(f),
            Value::Text(s) => json!(s),
            Value::Blob(b) => json!(format!("<blob {} bytes>", b.len())),
            Value::Timestamp(ts) => json!(ts),
        }
    }

    fn create_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let conversation_id = args["conversation_id"].as_i64().unwrap_or(0);
        let title = args["title"].as_str().unwrap_or("");
        let location = args["location"].as_str().unwrap_or("");

        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }

        let row = Row::new(vec![
            Value::Int64(conversation_id),
            Value::Text(title.to_string()),
            Value::Text(location.to_string()),
        ]);

        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        match db.insert("conversation", row) {
            Ok(_) => Ok(json!({
                "success": true,
                "conversation_id": conversation_id,
                "title": title
            }).to_string()),
            Err(e) => Ok(format!("Error creating conversation: {}", e)),
        }
    }

    fn list_conversations(&self, _args: &serde_json::Value) -> Result<String> {
        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        match db.scan("conversation") {
            Ok(rows) => {
                let conversations: Vec<_> = rows.iter().map(|row| {
                    json!({
                        "conversation_id": Self::value_to_json(&row.values[0]),
                        "title": Self::value_to_json(&row.values[1]),
                        "location": Self::value_to_json(&row.values[2])
                    })
                }).collect();

                Ok(json!({
                    "conversations": conversations,
                    "count": conversations.len()
                }).to_string())
            }
            Err(e) => Ok(format!("Error listing conversations: {}", e)),
        }
    }

    fn retrieve_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("");

        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }

        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        // Note: This does a full table scan. Once we add index support to the DB,
        // we should create an index on the title column for efficient lookups.
        match db.scan("conversation") {
            Ok(rows) => {
                for row in rows {
                    if let Value::Text(row_title) = &row.values[1] {
                        if row_title == title {
                            return Ok(json!({
                                "conversation_id": Self::value_to_json(&row.values[0]),
                                "title": Self::value_to_json(&row.values[1]),
                                "location": Self::value_to_json(&row.values[2])
                            }).to_string());
                        }
                    }
                }
                Ok(json!({
                    "error": "Conversation not found",
                    "title": title
                }).to_string())
            }
            Err(e) => Ok(format!("Error retrieving conversation: {}", e)),
        }
    }
}