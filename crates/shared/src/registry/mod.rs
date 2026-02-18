use once_cell::sync::Lazy;
use std::collections::HashMap;
use anyhow::Result;
use serde_json::Value;

use crate::schemas::{ToolHandler, ToolSchema, Tool, ToolLocation};
use crate::toolbelts::{file_smith, archivist, web_search};

static TOOL_REGISTRY: Lazy<HashMap<&'static str, ToolHandler>> = Lazy::new(|| {
    let mut map = HashMap::new();

    for (name, handler) in file_smith::TOOL_ENTRIES {
        map.insert(*name, *handler);
    }
    for (name, handler) in archivist::TOOL_ENTRIES {
        map.insert(*name, *handler);
    }
    for (name, handler) in web_search::TOOL_ENTRIES {
        map.insert(*name, *handler);
    }

    map
});

static TOOL_SCHEMAS: Lazy<Vec<ToolSchema>> = Lazy::new(|| {
    let mut schemas = Vec::new();
    schemas.extend(file_smith::TOOL_SCHEMAS.iter().cloned());
    schemas.extend(archivist::TOOL_SCHEMAS.iter().cloned());
    schemas.extend(web_search::TOOL_SCHEMAS.iter().cloned()); 
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