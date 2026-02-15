use anyhow::Result;
use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;

use super::toolbelts::{archivist, file_smith};
use super::{ToolSchema, Tool};

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

/// Get tools matching any of the given name prefixes
pub fn get_tools_for(prefixes: &[&str]) -> Vec<Tool> {
    TOOL_SCHEMAS
        .iter()
        .filter(|s| prefixes.iter().any(|p| s.name.starts_with(p)))
        .map(|s| s.to_tool())
        .collect()
}

pub fn get_tools_for_specialist(specialist: &crate::task::specialist::Specialist) -> Vec<Tool> {
    use crate::task::specialist::Specialist;

    match specialist {
        Specialist::ToolCaller => get_tools(), // All tools
        Specialist::Coder => get_tools_for(&["FileSmith"]),
        Specialist::Reasoner | Specialist::Quick => vec![], // No tools
    }
}

#[macro_export]
macro_rules! register_toolbelt {
    (
        $toolbelt_type:ty {
            description: $toolbelt_desc:literal,
            tools: {
                $(
                    $name:literal => $method:ident {
                        description: $desc:literal,
                        params: [$($param_name:literal: $param_type:literal => $param_desc:literal),* $(,)?]
                    }
                ),* $(,)?
            }
        }
    ) => {
        use once_cell::sync::Lazy;

        // Lazy singleton instance
        pub static INSTANCE: Lazy<$toolbelt_type> = Lazy::new(<$toolbelt_type>::default);

        // Generate wrapper functions that call the singleton
        $(
            paste::paste! {
                pub fn [<$method _handler>](args: &serde_json::Value) -> anyhow::Result<String> {
                    INSTANCE.$method(args)
                }
            }
        )*

        // Tool entries for registry (namespaced: "TypeName::tool_name")
        paste::paste! {
            pub static TOOL_ENTRIES: &[(&str, $crate::tools::ToolHandler)] = &[
                $((concat!(stringify!($toolbelt_type), "::", $name), [<$method _handler>])),*
            ];
        }

        // Tool schemas for LLM consumption
        pub static TOOL_SCHEMAS: Lazy<Vec<$crate::tools::ToolSchema>> = Lazy::new(|| vec![
            $(
                $crate::tools::ToolSchema {
                    name: concat!(stringify!($toolbelt_type), "::", $name),
                    description: $desc,
                    parameters: vec![
                        $(
                            $crate::tools::ParameterSchema {
                                name: $param_name,
                                type_name: $param_type,
                                description: $param_desc,
                                required: true,
                            }
                        ),*
                    ],
                }
            ),*
        ]);
    };
}
