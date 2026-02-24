use crate::{register_toolbelt, ToolLocation};

pub struct Memory;

impl Default for Memory {
    fn default() -> Self { Self }
}

register_toolbelt! {
    Memory {
        description: "Memory management - commit and recall information learned during execution",
        location: ToolLocation::Server,
        tools: {
            "commit" => commit {
                description: "Commit information to long-term memory for future sessions",
                params: [
                    "key": "string" => "Unique identifier for this memory (e.g., 'project_root', 'preferred_editor')",
                    "value": "string" => "The information to remember",
                    "memory_type": "string" => "Type: 'fact' (objective info), 'preference' (user choices), or 'context' (current situation)",
                    "confidence": "number" => "Confidence level 0.0-1.0 (default 1.0)"
                ]
            },
            "recall" => recall {
                description: "Retrieve a specific memory by key",
                params: [
                    "key": "string" => "The key to look up"
                ]
            },
            "search" => search {
                description: "Search memories by keyword or pattern",
                params: [
                    "pattern": "string" => "Search pattern (case-insensitive substring match)"
                ]
            }
        }
    }
}

impl Memory {
    fn commit(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        // This will be called with AgentContext injected
        // For now, return a marker that the agent's dispatch will handle
        Ok(format!("MEMORY_COMMIT:{}", serde_json::to_string(args)?))
    }

    fn recall(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        Ok(format!("MEMORY_RECALL:{}", serde_json::to_string(args)?))
    }

    fn search(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        Ok(format!("MEMORY_SEARCH:{}", serde_json::to_string(args)?))
    }
}