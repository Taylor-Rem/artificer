use crate::register_toolbelt;
use crate::traits::{ParameterSchema, ToolSchema};
use anyhow::Result;
use db::MiniDB;
use serde_json::json;
use std::sync::Mutex;

pub struct Archivist {
    db: Mutex<MiniDB>,
}

impl Default for Archivist {
    fn default() -> Self {
        let db_path = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("artificer")
            .join("preferences.db");

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let db = MiniDB::open(db_path.to_str().unwrap_or("preferences.db"))
            .expect("Failed to open preferences database");



        Self { db: Mutex::new(db) }
    }
}

register_toolbelt! {
    Archivist {
        description: "Tool for storing and retrieving user chat history and preferences",
        tools: {
            "set_preference" => set_preference {
                description: "Stores a user preference. The value can be any string (JSON recommended for complex data).",
                params: [
                    "key": "string" => "The preference key (e.g., 'theme', 'language')",
                    "value": "string" => "The preference value to store"
                ]
            },
            "get_preference" => get_preference {
                description: "Retrieves a stored user preference by key. Returns null if not found.",
                params: ["key": "string" => "The preference key to retrieve"]
            },
            "delete_preference" => delete_preference {
                description: "Deletes a stored user preference by key.",
                params: ["key": "string" => "The preference key to delete"]
            },
            "list_preferences" => list_preferences {
                description: "Lists all stored preference keys.",
                params: []
            },
        }
    }
}

impl Archivist {
    fn set_preference(&self, args: &serde_json::Value) -> Result<String> {
        let key = args["key"].as_str().unwrap_or("");
        let value = args["value"].as_str().unwrap_or("");

        if key.is_empty() {
            return Ok("Error: key cannot be empty".to_string());
        }

        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        match db.insert(key, value) {
            Ok(_) => Ok(format!("Successfully stored preference '{}'", key)),
            Err(e) => Ok(format!("Error storing preference: {}", e)),
        }
    }

    fn get_preference(&self, args: &serde_json::Value) -> Result<String> {
        let key = args["key"].as_str().unwrap_or("");

        if key.is_empty() {
            return Ok("Error: key cannot be empty".to_string());
        }

        let db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        match db.get(key) {
            Ok(Some(value)) => {
                let value_str = String::from_utf8_lossy(&value);
                Ok(json!({
                    "key": key,
                    "value": value_str
                }).to_string())
            }
            Ok(None) => Ok(json!({
                "key": key,
                "value": null
            }).to_string()),
            Err(e) => Ok(format!("Error retrieving preference: {}", e)),
        }
    }

    fn delete_preference(&self, args: &serde_json::Value) -> Result<String> {
        let key = args["key"].as_str().unwrap_or("");

        if key.is_empty() {
            return Ok("Error: key cannot be empty".to_string());
        }

        let mut db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        match db.remove(key) {
            Ok(true) => Ok(format!("Successfully deleted preference '{}'", key)),
            Ok(false) => Ok(format!("Preference '{}' not found", key)),
            Err(e) => Ok(format!("Error deleting preference: {}", e)),
        }
    }

    fn list_preferences(&self, _args: &serde_json::Value) -> Result<String> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        match db.keys() {
            Ok(keys) => {
                let key_strings: Vec<String> = keys
                    .iter()
                    .map(|k| String::from_utf8_lossy(k).to_string())
                    .collect();
                Ok(json!({
                    "keys": key_strings,
                    "count": key_strings.len()
                }).to_string())
            }
            Err(e) => Ok(format!("Error listing preferences: {}", e)),
        }
    }
}
