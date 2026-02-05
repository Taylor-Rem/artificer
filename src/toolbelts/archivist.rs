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

        // NEW: Create conversation table with auto-increment ID
        if db.get_schema("conversation").is_none() {
            let schema = TableBuilder::new("conversation")
                .column_auto_increment("conversation_id", DataType::Int64)  // AUTO-INCREMENT!
                .column_not_null("title", DataType::Text)
                .column("location", DataType::Text)
                .primary_key(&["conversation_id"])
                .build();

            db.create_table(schema).expect("Failed to create conversation table");
        }

        // Create index on title column for efficient lookups
        if db.get_schema("conversation")
            .and_then(|s| s.get_index("idx_title"))
            .is_none()
        {
            db.create_index("conversation", "idx_title", &["title"], false)
                .expect("Failed to create title index");
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
                description: "Creates a new conversation with auto-generated ID",
                params: [
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

    // NEW: Simplified - no need to pass conversation_id!
    fn create_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("");
        let location = args["location"].as_str().unwrap_or("");

        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }

        // NEW: Use Value::Null for auto-increment column
        let row = Row::new(vec![
            Value::Null,  // conversation_id will be auto-generated
            Value::Text(title.to_string()),
            Value::Text(location.to_string()),
        ]);

        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        match db.insert("conversation", row) {
            Ok(_) => {
                // Get the auto-generated ID by retrieving the schema
                let schema = db.get_schema("conversation").unwrap();
                let generated_id = schema.auto_increment - 1;  // Last used ID

                Ok(json!({
                    "success": true,
                    "conversation_id": generated_id,
                    "title": title
                }).to_string())
            }
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

        // Use index lookup for efficient retrieval by title
        match db.find_by_index("conversation", "idx_title", &[Value::Text(title.to_string())]) {
            Ok(rows) => {
                if let Some(row) = rows.first() {
                    Ok(json!({
                        "conversation_id": Self::value_to_json(&row.values[0]),
                        "title": Self::value_to_json(&row.values[1]),
                        "location": Self::value_to_json(&row.values[2])
                    }).to_string())
                } else {
                    Ok(json!({
                        "error": "Conversation not found",
                        "title": title
                    }).to_string())
                }
            }
            Err(e) => Ok(format!("Error retrieving conversation: {}", e)),
        }
    }
}