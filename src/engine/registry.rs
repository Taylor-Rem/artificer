use anyhow::Result;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;

use crate::toolbelts::{archivist, file_smith};
use crate::schema::{ToolSchema, Tool};

type Handler = fn(&Value) -> Result<String>;

static TOOL_REGISTRY: Lazy<HashMap<&'static str, Handler>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // Register all toolbelts here
    for (name, handler) in archivist::TOOL_ENTRIES {
        map.insert(*name, *handler);
    }
    for (name, handler) in file_smith::TOOL_ENTRIES {
        map.insert(*name, *handler);
    }

    map
});

static TOOL_SCHEMAS: Lazy<Vec<ToolSchema>> = Lazy::new(|| {
    let mut schemas = Vec::new();

    // Collect schemas from all toolbelts
    schemas.extend(archivist::TOOL_SCHEMAS.iter().cloned());
    schemas.extend(file_smith::TOOL_SCHEMAS.iter().cloned());

    schemas
});

pub fn use_tool(name: &str, args: &Value) -> Result<String> {
    TOOL_REGISTRY
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found", name))
        .and_then(|handler| handler(args))
}

/// Get all tools in Ollama/OpenAI format
pub fn get_tools() -> Vec<Tool> {
    TOOL_SCHEMAS.iter().map(|s| s.to_tool()).collect()
}
