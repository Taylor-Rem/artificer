use once_cell::sync::Lazy;
use artificer_shared::schemas::{ToolSchema, ParameterSchema, ToolLocation};

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
            ParameterSchema {
                name: "execution_type",
                type_name: "string",
                description: "How the specialist should execute: 'tool_proxy' returns raw tool results with no synthesis, 'agentic' runs the full reasoning loop and returns a conclusion.",
                required: true,
            },
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
            ParameterSchema {
                name: "execution_type",
                type_name: "string",
                description: "How the specialist should execute: 'tool_proxy' returns raw tool results with no synthesis, 'agentic' runs the full reasoning loop and returns a conclusion.",
                required: true,
            },
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
            ParameterSchema {
                name: "execution_type",
                type_name: "string",
                description: "How the specialist should execute: 'tool_proxy' returns raw tool results with no synthesis, 'agentic' runs the full reasoning loop and returns a conclusion.",
                required: true,
            },
        ],
    },
]);
