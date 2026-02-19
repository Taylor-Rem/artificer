use once_cell::sync::Lazy;
use std::collections::HashMap;
use anyhow::Result;
use serde_json::Value;

use crate::schemas::{Tool, ToolHandler, ToolLocation, ToolSchema};

pub mod toolbelts;

static TOOL_REGISTRY: Lazy<HashMap<&'static str, ToolHandler>> = Lazy::new(|| {
    let mut map = HashMap::new();

    for (name, handler) in toolbelts::file_smith::TOOL_ENTRIES { map.insert(*name, *handler); }
    for (name, handler) in toolbelts::archivist::TOOL_ENTRIES { map.insert(*name, *handler); }
    for (name, handler) in toolbelts::web_search::TOOL_ENTRIES { map.insert(*name, *handler); }
    for (name, handler) in toolbelts::router::TOOL_ENTRIES { map.insert(*name, *handler); }
    map
});

static TOOL_SCHEMAS: Lazy<Vec<ToolSchema>> = Lazy::new(|| {
    let mut schemas = Vec::new();
    schemas.extend(toolbelts::file_smith::TOOL_SCHEMAS.iter().cloned());
    schemas.extend(toolbelts::archivist::TOOL_SCHEMAS.iter().cloned());
    schemas.extend(toolbelts::web_search::TOOL_SCHEMAS.iter().cloned());
    schemas.extend(toolbelts::router::TOOL_SCHEMAS.iter().cloned());
    schemas
});

pub fn use_tool(name: &str, args: &Value) -> Result<String> {
    TOOL_REGISTRY
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found", name))
        .and_then(|handler| handler(args))
}

pub fn get_tools() -> Vec<Tool> {
    TOOL_SCHEMAS.iter().map(|s| s.to_tool()).collect()
}

pub fn get_tools_for(prefixes: &[&str]) -> Vec<Tool> {
    TOOL_SCHEMAS
        .iter()
        .filter(|s| prefixes.iter().any(|p| s.name.starts_with(p)))
        .map(|s| s.to_tool())
        .collect()
}

pub fn get_server_tools() -> Vec<Tool> {
    TOOL_SCHEMAS
        .iter()
        .filter(|s| matches!(s.location, ToolLocation::Server))
        .map(|s| s.to_tool())
        .collect()
}

pub fn get_client_tools() -> Vec<Tool> {
    TOOL_SCHEMAS
        .iter()
        .filter(|s| matches!(s.location, ToolLocation::Client))
        .map(|s| s.to_tool())
        .collect()
}

pub fn get_tool_schema(name: &str) -> anyhow::Result<&'static ToolSchema> {
    TOOL_SCHEMAS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| anyhow::anyhow!("Tool schema '{}' not found", name))
}