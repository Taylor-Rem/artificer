use crate::register_toolbelt;
use crate::traits::{ParameterSchema, ToolSchema};
use anyhow::Result;
use redb::{Database, ReadableTable, TableDefinition};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

// Table definitions
const PREFERENCES: TableDefinition<&str, &str> = TableDefinition::new("preferences");
const FACTS: TableDefinition<u64, &str> = TableDefinition::new("facts");
const COUNTERS: TableDefinition<&str, u64> = TableDefinition::new("counters");

static DB: once_cell::sync::Lazy<Arc<Database>> = once_cell::sync::Lazy::new(|| {
    let db = init_db().expect("Failed to initialize database");
    Arc::new(db)
});

fn get_db_path() -> PathBuf {
    let dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    dir.join(".artificer")
}

fn init_db() -> Result<Database> {
    let db_dir = get_db_path();
    std::fs::create_dir_all(&db_dir)?;

    let db_path = db_dir.join("archivist.redb");
    let db = Database::create(&db_path)?;

    // Initialize tables by opening a write transaction
    let write_txn = db.begin_write()?;
    {
        // Create tables if they don't exist
        let _ = write_txn.open_table(PREFERENCES)?;
        let _ = write_txn.open_table(FACTS)?;
        let _ = write_txn.open_table(COUNTERS)?;
    }
    write_txn.commit()?;

    Ok(db)
}

/// Returns stored user preferences and facts formatted for the system prompt
pub fn get_user_context() -> String {
    let _ = &*DB; // Ensure DB is initialized

    let mut parts = Vec::new();

    // Get preferences
    if let Ok(read_txn) = DB.begin_read() {
        if let Ok(table) = read_txn.open_table(PREFERENCES) {
            let prefs: Vec<String> = table
                .iter()
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.ok())
                .map(|(k, v)| format!("{}: {}", k.value(), v.value()))
                .collect();

            if !prefs.is_empty() {
                parts.push(format!("User preferences:\n{}", prefs.join("\n")));
            }
        }
    }

    // Get facts
    if let Ok(read_txn) = DB.begin_read() {
        if let Ok(table) = read_txn.open_table(FACTS) {
            let facts: Vec<String> = table
                .iter()
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.ok())
                .filter_map(|(_, v)| {
                    serde_json::from_str::<serde_json::Value>(v.value())
                        .ok()
                        .and_then(|j| j["fact"].as_str().map(|s| s.to_string()))
                })
                .collect();

            if !facts.is_empty() {
                parts.push(format!("Known facts about user:\n- {}", facts.join("\n- ")));
            }
        }
    }

    if parts.is_empty() {
        "No stored preferences or facts.".to_string()
    } else {
        parts.join("\n\n")
    }
}

pub struct Archivist;

impl Default for Archivist {
    fn default() -> Self {
        // Ensure DB is initialized
        let _ = &*DB;
        Self
    }
}

register_toolbelt! {
    Archivist {
        description: "Archives and retrieves user preferences and learned facts",
        tools: {
            "set_preference" => set_preference {
                description: "Sets a user preference (key-value pair). Overwrites if key exists.",
                params: [
                    "key": "string" => "The preference key (e.g., 'username', 'response_style')",
                    "value": "string" => "The preference value"
                ]
            },
            "get_preference" => get_preference {
                description: "Gets a user preference by key. Returns null if not found.",
                params: ["key": "string" => "The preference key to retrieve"]
            },
            "list_preferences" => list_preferences {
                description: "Lists all stored user preferences.",
                params: []
            },
            "delete_preference" => delete_preference {
                description: "Deletes a user preference by key.",
                params: ["key": "string" => "The preference key to delete"]
            },
            "save_fact" => save_fact {
                description: "Saves a learned fact about the user. Use for things the AI discovers during conversation.",
                params: [
                    "fact": "string" => "The fact to remember (e.g., 'prefers concise responses')",
                    "category": "string" => "Optional category (e.g., 'coding', 'communication', 'personal')"
                ]
            },
            "get_facts" => get_facts {
                description: "Retrieves all learned facts, optionally filtered by category.",
                params: ["category": "string" => "Optional category to filter by (leave empty for all)"]
            },
            "delete_fact" => delete_fact {
                description: "Deletes a learned fact by its ID.",
                params: ["id": "integer" => "The fact ID to delete"]
            }
        }
    }
}

impl Archivist {
    fn set_preference(&self, args: &serde_json::Value) -> Result<String> {
        let key = args["key"].as_str().unwrap_or("");
        let value = args["value"].as_str().unwrap_or("");

        if key.is_empty() {
            return Ok("Error: key is required".to_string());
        }

        let write_txn = DB.begin_write()?;
        {
            let mut table = write_txn.open_table(PREFERENCES)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;

        Ok(format!("Preference '{}' set to '{}'", key, value))
    }

    fn get_preference(&self, args: &serde_json::Value) -> Result<String> {
        let key = args["key"].as_str().unwrap_or("");

        let read_txn = DB.begin_read()?;
        let table = read_txn.open_table(PREFERENCES)?;
        let result = table.get(key)?;

        match result {
            Some(value) => Ok(json!({"key": key, "value": value.value()}).to_string()),
            None => Ok(json!({"key": key, "value": null}).to_string()),
        }
    }

    fn list_preferences(&self, _args: &serde_json::Value) -> Result<String> {
        let read_txn = DB.begin_read()?;
        let table = read_txn.open_table(PREFERENCES)?;

        let mut prefs: Vec<serde_json::Value> = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            prefs.push(json!({
                "key": key.value(),
                "value": value.value()
            }));
        }

        // Sort by key for consistent ordering
        prefs.sort_by(|a, b| {
            a["key"].as_str().unwrap_or("").cmp(b["key"].as_str().unwrap_or(""))
        });

        Ok(json!({"preferences": prefs, "count": prefs.len()}).to_string())
    }

    fn delete_preference(&self, args: &serde_json::Value) -> Result<String> {
        let key = args["key"].as_str().unwrap_or("");

        let write_txn = DB.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(PREFERENCES)?;
            table.remove(key)?.is_some()
        };
        write_txn.commit()?;

        if removed {
            Ok(format!("Deleted preference '{}'", key))
        } else {
            Ok(format!("Preference '{}' not found", key))
        }
    }

    fn save_fact(&self, args: &serde_json::Value) -> Result<String> {
        let fact = args["fact"].as_str().unwrap_or("");
        let category = args["category"].as_str().filter(|s| !s.is_empty());

        if fact.is_empty() {
            return Ok("Error: fact is required".to_string());
        }

        // Check for duplicate facts first
        {
            let read_txn = DB.begin_read()?;
            let table = read_txn.open_table(FACTS)?;
            for entry in table.iter()? {
                let (_, value) = entry?;
                if let Ok(stored) = serde_json::from_str::<serde_json::Value>(value.value()) {
                    if stored["fact"].as_str() == Some(fact) {
                        return Ok(format!("Fact already exists: '{}'", fact));
                    }
                }
            }
        }

        let write_txn = DB.begin_write()?;
        {
            // Get next ID from counter
            let mut counters = write_txn.open_table(COUNTERS)?;
            let next_id = counters
                .get("facts_counter")?
                .map(|v| v.value() + 1)
                .unwrap_or(1);
            counters.insert("facts_counter", next_id)?;

            // Store fact as JSON
            let fact_json = json!({
                "fact": fact,
                "category": category
            });
            let mut facts = write_txn.open_table(FACTS)?;
            facts.insert(next_id, fact_json.to_string().as_str())?;
        }
        write_txn.commit()?;

        Ok(format!("Saved fact: '{}'", fact))
    }

    fn get_facts(&self, args: &serde_json::Value) -> Result<String> {
        let category = args["category"].as_str().filter(|s| !s.is_empty());

        let read_txn = DB.begin_read()?;
        let table = read_txn.open_table(FACTS)?;

        let mut facts: Vec<serde_json::Value> = Vec::new();
        for entry in table.iter()? {
            let (id, value) = entry?;
            if let Ok(mut stored) = serde_json::from_str::<serde_json::Value>(value.value()) {
                // Filter by category if specified
                if let Some(cat) = category {
                    if stored["category"].as_str() != Some(cat) {
                        continue;
                    }
                }
                // Add the ID to the result
                stored["id"] = json!(id.value());
                facts.push(stored);
            }
        }

        // Reverse to show newest first (highest ID first)
        facts.reverse();

        Ok(json!({"facts": facts, "count": facts.len()}).to_string())
    }

    fn delete_fact(&self, args: &serde_json::Value) -> Result<String> {
        let id = args["id"].as_u64().unwrap_or(0);

        let write_txn = DB.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(FACTS)?;
            table.remove(id)?.is_some()
        };
        write_txn.commit()?;

        if removed {
            Ok(format!("Deleted fact with ID {}", id))
        } else {
            Ok(format!("Fact with ID {} not found", id))
        }
    }
}
