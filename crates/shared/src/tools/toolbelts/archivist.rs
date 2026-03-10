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
        description: "Tool for managing chat history. All queries are automatically scoped to the current device.",
        location: ToolLocation::Server,
        tools: {
            "query_db" => query_db {
                description: "Runs a SQL query against the database.",
                params: ["query": "string" => "SQL query string", "params": "array" => "Ordered parameter values for ?1, ?2, etc."]
            },
            "list_tables" => list_tables {
                description: "Lists all tables and views in the database",
                params: []
            },
            "list_conversations" => list_conversations {
                description: "Lists all conversations for the current device with their IDs and titles",
                params: []
            },
            "get_conversation" => get_conversation {
                description: "Retrieves a conversation and all messages by title for the current device",
                params: ["title": "string" => "Title of the conversation to retrieve"]
            },
            "get_task_trace" => get_task_trace {
                description: "Get the execution trace for a task showing every LLM iteration, what the model reasoned, what tools it called, and how each iteration was classified. Use this to debug agent behavior.",
                params: ["task_id": "integer" => "The task ID to get traces for"]
            },
            "get_trace_detail" => get_trace_detail {
                description: "Get the full detail for a specific iteration of a task trace, including the complete input context that was sent to the model. Use for deep debugging of a specific decision.",
                params: [
                    "task_id": "integer" => "The task ID",
                    "iteration": "integer" => "The iteration number to inspect"
                ]
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
            "SELECT id, title, created, last_accessed
             FROM conversations
             ORDER BY last_accessed DESC",
            rusqlite::params![],
        )
    }

    fn get_task_trace(&self, args: &serde_json::Value) -> Result<String> {
        let task_id = args["task_id"].as_u64().unwrap_or(0);
        if task_id == 0 {
            return Ok("Error: task_id is required".to_string());
        }
        db::get().get_execution_traces(task_id)
    }

    fn get_trace_detail(&self, args: &serde_json::Value) -> Result<String> {
        let task_id = args["task_id"].as_u64().unwrap_or(0);
        let iteration = args["iteration"].as_u64().unwrap_or(0) as u32;
        if task_id == 0 || iteration == 0 {
            return Ok("Error: task_id and iteration are required".to_string());
        }
        db::get().get_execution_trace_detail(task_id, iteration)
    }

    fn get_conversation(&self, args: &serde_json::Value) -> Result<String> {
        let title = args["title"].as_str().unwrap_or("");
        if title.is_empty() {
            return Ok("Error: title cannot be empty".to_string());
        }

        let conv_result = db::get().query(
            "SELECT id, title FROM conversations WHERE title = ?1",
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
        output.push_str("\nmessages:\n");

        let messages_result = db::get().query(
            "SELECT role, message FROM messages
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
}
