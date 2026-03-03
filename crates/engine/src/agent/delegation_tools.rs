use once_cell::sync::Lazy;
use artificer_shared::schemas::{ToolSchema, ParameterSchema, ToolLocation};

fn synthesize_param() -> ParameterSchema {
    ParameterSchema {
        name: "synthesize",
        type_name: "boolean",
        description: "When true, the specialist may return a synthesized text response (analysis, summary, explanation). \
                      When false (default), raw tool results are returned verbatim — use this for retrieval tasks.",
        required: false,
    }
}

pub static DELEGATION_TOOLS: Lazy<Vec<ToolSchema>> = Lazy::new(|| vec![
    ToolSchema {
        name: "delegate::file_smith",
        description: "Delegate file system operations to FileSmith specialist. Use for reading, writing, or manipulating files.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "goal",
                type_name: "string",
                description: "What you need FileSmith to do",
                required: true,
            },
            synthesize_param(),
        ],
    },
    ToolSchema {
        name: "delegate::web_researcher",
        description: "Delegate web research to WebResearcher specialist. Use for searching the web or fetching pages.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "goal",
                type_name: "string",
                description: "What you need WebResearcher to do",
                required: true,
            },
            synthesize_param(),
        ],
    },
    ToolSchema {
        name: "delegate::archivist",
        description: "Delegate database and conversation history queries to Archivist specialist.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "goal",
                type_name: "string",
                description: "What you need Archivist to do",
                required: true,
            },
            synthesize_param(),
        ],
    },
]);
