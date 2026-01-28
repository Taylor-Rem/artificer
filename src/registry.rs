use anyhow::Result;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;

use crate::toolbelts::file_smith;

type Handler = fn(&Value) -> Result<String>;

static TOOL_REGISTRY: Lazy<HashMap<&'static str, Handler>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // Register all toolbelts here
    for (name, handler) in file_smith::TOOL_ENTRIES {
        map.insert(*name, *handler);
    }

    map
});

pub fn use_tool(name: &str, args: &Value) -> Result<String> {
    TOOL_REGISTRY
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found", name))
        .and_then(|handler| handler(args))
}
