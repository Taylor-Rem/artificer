use anyhow::Result;
use serde_json::json;

use crate::register_toolbelt;
use crate::ToolLocation;
use crate::db;

pub struct Archivist;

impl Default for Archivist {
    fn default() -> Self {
        Self
    }
}

register_toolbelt! {
    Archivist {
        description: "Tool for managing chat history, user memory, and preferences",
        location: ToolLocation::Server,
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

fn sanitize_title(title: &str) -> String {
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

impl Archivist {
    fn query_db(&self, args: &serde_json::Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return Ok("Error: query cannot be empty".to_string());
        }
        let params = args["params"]
            .as_array()
            .map(|arr| arr.clone())
            .unwrap_or_default();
        db::query(query, params)
    }

    fn list_tables(&self, _args: &serde_json::Value) -> Result<String> {
        db::query(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
            vec![],
        )
    }

    fn list_conversations(&self, _args: &serde_json::Value) -> Result<String> {
        db::query(
            "SELECT id, title, created, last_accessed FROM conversations ORDER BY last_accessed DESC",
            vec![],
        )
    }

    fn get_summary(&self, args: &serde_json::Value) -> Result<String> {
        let title = sanitize_title(args["title"].as_str().unwrap_or(""));
        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }
        db::query(
            "SELECT summary FROM conversations WHERE title = ?1",
            vec![json!(title)],
        )
    }

    fn get_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("");

        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }

        // Get conversation metadata
        let conv_result = db::query(
            "SELECT id, title FROM conversations WHERE title = ?1",
            vec![json!(title)],
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

        // Get tasks used in this conversation
        let tasks_result = db::query(
            "SELECT DISTINCT t.title
             FROM task_history th
             JOIN tasks t ON th.task_id = t.id
             WHERE th.conversation_id = ?1
             ORDER BY th.created",
            vec![json!(conv_id)],
        )?;

        let tasks: Vec<serde_json::Value> = serde_json::from_str(&tasks_result)?;
        if !tasks.is_empty() {
            let task_names: Vec<&str> = tasks.iter()
                .filter_map(|t| t["title"].as_str())
                .collect();
            output.push_str(&format!("tasks_used: {}\n", task_names.join(", ")));
        }

        output.push_str("\nmessages:\n");

        // Get all messages for this conversation
        let messages_result = db::query(
            "SELECT role, message FROM messages
             WHERE conversation_id = ?1
             ORDER BY m_order",
            vec![json!(conv_id)],
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
}
