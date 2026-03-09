use once_cell::sync::Lazy;
use artificer_shared::schemas::{ToolSchema, ParameterSchema, ToolLocation};
use serde_json::Value;
use anyhow::Result;
use crate::agent::execution::specialist_state::SpecialistState;

pub static SPECIALIST_CONTROL_TOOLS: Lazy<Vec<ToolSchema>> = Lazy::new(|| vec![
    ToolSchema {
        name: "response::return_with_tool_call",
        description: "Add a tool call result to the response buffer by index AND return to the orchestrator immediately. Use this when a single result fulfills the request.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "index",
                type_name: "integer",
                description: "The index of the tool call to include in the response",
                required: true,
            },
            ParameterSchema {
                name: "message",
                type_name: "string",
                description: "Brief summary for the orchestrator describing what was done",
                required: false,
            },
        ],
    },
    ToolSchema {
        name: "response::add_to_response",
        description: "Add a tool call result to the response buffer by index WITHOUT returning. Use this when you need to collect multiple results before returning.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "index",
                type_name: "integer",
                description: "The index of the tool call to include in the response",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "response::return_as_is",
        description: "Return to the orchestrator with the current contents of response_vec. Use when response_vec already contains everything needed.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "message",
                type_name: "string",
                description: "Brief summary for the orchestrator describing what was done",
                required: false,
            },
        ],
    },
    ToolSchema {
        name: "response::get_full_result",
        description: "Retrieve the full, untruncated result of a tool call by index. Use this when the truncated preview in <tool_calls> isn't sufficient to verify the result.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "index",
                type_name: "integer",
                description: "The index of the tool call whose full result you want to see",
                required: true,
            },
        ],
    },
]);

pub fn is_specialist_control_tool(name: &str) -> bool {
    name.starts_with("response::")
}

pub fn is_return_triggering_tool(name: &str) -> bool {
    matches!(name,
        "response::return_with_tool_call" |
        "response::return_as_is"
    )
}

pub fn handle_specialist_control_tool(
    state: &mut SpecialistState,
    tool_name: &str,
    args: &Value,
) -> Result<String> {
    match tool_name {
        "response::return_with_tool_call" => {
            let index = args["index"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'index' parameter"))? as usize;
            let message = args["message"].as_str().map(String::from);
            if let Some(msg) = message {
                state.set_response_message(msg);
            }
            state.return_with_tool_call(index)
                .map_err(|e| anyhow::anyhow!(e))
        }
        "response::add_to_response" => {
            let index = args["index"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'index' parameter"))? as usize;
            state.add_to_response_vec(index)
                .map_err(|e| anyhow::anyhow!(e))
        }
        "response::return_as_is" => {
            let message = args["message"].as_str().map(String::from);
            if let Some(msg) = message {
                state.set_response_message(msg);
            }
            Ok(state.return_as_is())
        }
        "response::get_full_result" => {
            let index = args["index"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'index' parameter"))? as usize;
            state.get_full_result(index)
                .map_err(|e| anyhow::anyhow!(e))
        }
        _ => Err(anyhow::anyhow!("Unknown specialist control tool: {}", tool_name)),
    }
}
