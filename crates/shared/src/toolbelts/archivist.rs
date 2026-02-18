// crates/shared/src/toolbelts/archivist.rs
use anyhow::Result;
use crate::{register_toolbelt, ToolLocation, db};

pub struct Archivist;

impl Default for Archivist {
    fn default() -> Self {
        Self
    }
}

register_toolbelt! {
    Archivist {
        description: "Tool for managing chat history, user memory, and preferences. All queries are automatically scoped to the current device.",
        location: ToolLocation::Server,
        tools: {
            "query_db" => query_db {
                description: "Runs a SQL query against device-scoped views. Use device_* views for device data. Global tables (tasks, keywords) can be queried directly.",
                params: ["query": "string" => "SQL query string", "params": "array" => "Ordered parameter values for ?1, ?2, etc."]
            },
            "list_tables" => list_tables {
                description: "Lists all tables and views in the database",
                params: []
            },
            "list_conversations" => list_conversations {
                description: "Lists all conversations for the current device with their IDs, titles, and keywords",
                params: []
            },
            "get_conversation" => get_conversation {
                description: "Retrieves a conversation and all messages by title for the current device",
                params: ["title": "string" => "Title of the conversation to retrieve"]
            },
            "search_by_keyword" => search_by_keyword {
                description: "Search conversations by keyword for the current device",
                params: ["keyword": "string" => "Keyword to search for (case-insensitive)"]
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

        // Device context should already be set by task execution
        let params_json = args["params"].as_array()
            .map(|arr| arr.clone())
            .unwrap_or_default();

        let params: Vec<rusqlite::types::Value> = params_json.iter()
            .map(|v| db::json_to_rusqlite(v))
            .collect();

        db::get().query(query, rusqlite::params_from_iter(params))
    }

    fn list_tables(&self, _args: &serde_json::Value) -> Result<String> {
        db::get().query(
            "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY type, name",
            rusqlite::params![],
        )
    }

    fn list_conversations(&self, _args: &serde_json::Value) -> Result<String> {
        db::get().query(
            "SELECT id, title, keywords, created, last_accessed
             FROM device_conversations_with_keywords
             ORDER BY last_accessed DESC",
            rusqlite::params![],
        )
    }

    fn get_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("");
        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }

        let conv_result = db::get().query(
            "SELECT id, title FROM device_conversations WHERE title = ?1",
            rusqlite::params![title],
        )?;

        let conversations: Vec<serde_json::Value> = serde_json::from_str(&conv_result)?;
        if conversations.is_empty() {
            return Ok(format!("Error: Conversation '{}' not found", title));
        }

        let conv = &conversations[0];
        let conv_id = conv["id"].as_i64().unwrap_or(0);
        let conv_title = conv["title"].as_str().unwrap_or("Untitled");

        let mut output = String::new();
        output.push_str(&format!("title: {}\n", conv_title));

        // Get tasks
        let tasks_result = db::get().query(
            "SELECT DISTINCT t.title
             FROM device_task_history th
             JOIN tasks t ON th.task_id = t.id
             WHERE th.conversation_id = ?1
             ORDER BY th.created",
            rusqlite::params![conv_id],
        )?;

        let tasks: Vec<serde_json::Value> = serde_json::from_str(&tasks_result)?;
        if !tasks.is_empty() {
            let task_names: Vec<&str> = tasks.iter()
                .filter_map(|t| t["title"].as_str())
                .collect();
            output.push_str(&format!("tasks_used: {}\n", task_names.join(", ")));
        }

        // Get keywords
        let keywords_result = db::get().query(
            "SELECT k.keyword
             FROM device_conversation_keywords ck
             JOIN keywords k ON ck.keyword_id = k.id
             WHERE ck.conversation_id = ?1",
            rusqlite::params![conv_id],
        )?;

        let keywords: Vec<serde_json::Value> = serde_json::from_str(&keywords_result)?;
        if !keywords.is_empty() {
            let keyword_list: Vec<&str> = keywords.iter()
                .filter_map(|kw| kw["keyword"].as_str())
                .collect();
            output.push_str(&format!("keywords: {}\n", keyword_list.join(", ")));
        }

        output.push_str("\nmessages:\n");

        // Get messages
        let messages_result = db::get().query(
            "SELECT role, message FROM device_messages
             WHERE conversation_id = ?1
             ORDER BY m_order",
            rusqlite::params![conv_id],
        )?;

        let messages: Vec<serde_json::Value> = serde_json::from_str(&messages_result)?;
        for msg in messages {
            let role = msg["role"].as_str().unwrap_or("");
            let content = msg["message"].as_str().unwrap_or("");
            output.push_str(&format!("\nrole: {}\n", role));
            output.push_str(&format!("message: {}\n", content));
        }

        Ok(output)
    }

    fn search_by_keyword(&self, args: &serde_json::Value) -> Result<String> {
        let keyword = args["keyword"].as_str().unwrap_or("").to_lowercase();
        if keyword.is_empty() {
            return Ok("Error: keyword cannot be empty".to_string());
        }

        db::get().query(
            "SELECT DISTINCT c.id, c.title, c.created, c.last_accessed
             FROM device_conversations c
             JOIN device_conversation_keywords ck ON c.id = ck.conversation_id
             JOIN keywords k ON ck.keyword_id = k.id
             WHERE k.keyword LIKE '%' || ?1 || '%'
             ORDER BY c.last_accessed DESC",
            rusqlite::params![keyword],
        )
    }
}