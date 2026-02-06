use crate::register_toolbelt;
use crate::traits::{ParameterSchema, ToolSchema};
use anyhow::Result;
use db::{Database, DataType, Value};
use db::TableBuilder;
use db::query_builder::{QueryBuilder, QueryResult};
use serde_json::json;
use std::sync::Mutex;

pub struct Archivist {
    db: Mutex<Database>,
}

impl Default for Archivist {
    fn default() -> Self {
        let db_path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("RustroverProjects")
            .join("artificer")
            .join("memory.db");

        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut db = if db_path.exists() {
            Database::open(&db_path).expect("Failed to open database")
        } else {
            Database::create(&db_path).expect("Failed to create database")
        };
        // conversations table
        if db.get_schema("conversation").is_none() {
            let schema = TableBuilder::new("conversation")
                .column_auto_increment("id", DataType::UInt64)
                .column_not_null("title", DataType::Text)
                .column("location", DataType::Text)
                .primary_key(&["id"])
                .build();

            db.create_table(schema).expect("Failed to create conversation table");
        }
        if db.get_schema("conversation")
            .and_then(|s| s.get_index("idx_title"))
            .is_none()
        {
            db.create_index("conversation", "idx_title", &["title"], false)
                .expect("Failed to create title index");
        }
        // messages table
        if db.get_schema("message").is_none() {
            let schema = TableBuilder::new("message")
                .column_auto_increment("id", DataType::UInt64)
                .column("conversation_id", DataType::UInt64)
                .column_not_null("role", DataType::Text)
                .column_not_null("message", DataType::Text)
                .column_not_null("order", DataType::UInt32)
                .primary_key(&["id"])
                .build();
            db.create_table(schema).expect("Failed to create message table");
        }
        if db.get_schema("message")
            .and_then(|s| s.get_index("conversation_id"))
            .is_none()
        {
            db.create_index("message", "idx_conversation_id", &["conversation_id"], false)
                    .expect("Failed to create conversation_id index");
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
            Value::UInt32(n) => json!(n),
            Value::UInt64(n) => json!(n),
            Value::Int32(n) => json!(n),
            Value::Int64(n) => json!(n),
            Value::Float64(f) => json!(f),
            Value::Text(s) => json!(s),
            Value::Blob(b) => json!(format!("<blob {} bytes>", b.len())),
            Value::Timestamp(ts) => json!(ts),
        }
    }

    pub fn create_conversation(&self, title: &str, location: &str) -> Result<u64> {
        if title.is_empty() {
            return Err(anyhow::anyhow!("title cannot be empty"));
        }

        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        QueryBuilder::new(&mut db)
            .from("conversation")
            .values(vec![
                Value::Null,
                Value::Text(title.to_string()),
                Value::Text(location.to_string()),
            ])
            .insert()?;

        let schema = db.get_schema("conversation")
            .ok_or_else(|| anyhow::anyhow!("conversation schema not found"))?;

        Ok(schema.auto_increment)
    }
    pub fn create_message(&self, conversation_id: u64, role: &str, message: &str, order: &u32) -> Result<String> {
        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        match QueryBuilder::new(&mut db)
            .from("message")
            .values(vec![
                Value::Null,
                Value::UInt64(conversation_id),
                Value::Text(role.to_string()),
                Value::Text(message.to_string()),
                Value::UInt32(*order),
            ])
            .insert()
        {
            Ok(_) => {
                let schema = db.get_schema("message").unwrap();
                Ok(json!({
                    "success": true,
                    "conversation_id": conversation_id,
                    "message_id": schema.auto_increment,
                    "role": role,
                    "message": message
                }).to_string())
            }
            Err(e) => Ok(format!("Error creating message: {}", e)),
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

        // Find the conversation by title
        let result = QueryBuilder::new(&mut db)
            .from("conversation")
            .where_eq("title", Value::Text(title.to_string()))
            .limit(1)
            .execute();

        match result {
            Ok(QueryResult::Simple(rows)) => {
                if let Some(row) = rows.first() {
                    let conv_id = row.values[0].clone();

                    // Fetch messages for this conversation
                    let messages_result = QueryBuilder::new(&mut db)
                        .from("message")
                        .where_eq("conversation_id", conv_id)
                        .execute();

                    let messages = match messages_result {
                        Ok(QueryResult::Simple(msg_rows)) => {
                            msg_rows.iter().map(|msg| {
                                json!({
                                  "message_id": Self::value_to_json(&msg.values[0]),
                                  "role": Self::value_to_json(&msg.values[2]),
                                  "message": Self::value_to_json(&msg.values[3]),
                              })
                            }).collect::<Vec<_>>()
                        }
                        _ => vec![],
                    };

                    Ok(json!({
                      "conversation_id": Self::value_to_json(&row.values[0]),
                      "title": Self::value_to_json(&row.values[1]),
                      "location": Self::value_to_json(&row.values[2]),
                      "messages": messages,
                  }).to_string())
                } else {
                    Ok(json!({
                      "error": "Conversation not found",
                      "title": title
                  }).to_string())
                }
            }
            Ok(_) => Ok("Error: unexpected query result type".to_string()),
            Err(e) => Ok(format!("Error retrieving conversation: {}", e)),
        }
    }

}