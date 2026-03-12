use once_cell::sync::Lazy;
use artificer_shared::schemas::{ToolSchema, ParameterSchema, ToolLocation};
use serde_json::Value;
use anyhow::Result;
use crate::agent::state::TaskState;

pub static TASK_TOOLS: Lazy<Vec<ToolSchema>> = Lazy::new(|| vec![
    ToolSchema {
        name: "task::set_agent_goal",
        description: "Set your interpretation of the user's goal. Call this early to clarify your understanding.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "goal",
                type_name: "string",
                description: "Your interpretation/refinement of the user's goal",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "task::set_plan",
        description: "Set your plan as an ordered list of steps.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "steps",
                type_name: "array",
                description: "Ordered list of step descriptions",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "task::set_iterations",
        description: "Declare how many iterations this task requires. Call this once at planning time for repetitive tasks (e.g. 'apply to 100 jobs' → total=100). Enables iteration tracking and loop-aware completion checks.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "total",
                type_name: "integer",
                description: "Total number of iterations required",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "task::complete_iteration",
        description: "Increment the completed iteration counter by 1. Call this after each successful iteration of a repetitive task.",
        location: ToolLocation::Server,
        parameters: vec![],
    },
    ToolSchema {
        name: "task::set_current_step",
        description: "Set which step you are currently working on.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "step",
                type_name: "string",
                description: "Description of the current step",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "task::mark_step_complete",
        description: "Mark the current step as complete.",
        location: ToolLocation::Server,
        parameters: vec![],
    },
    ToolSchema {
        name: "task::mark_complete",
        description: "Mark the entire task as complete. Only call this when the goal is fully achieved.",
        location: ToolLocation::Server,
        parameters: vec![],
    },
    ToolSchema {
        name: "task::set_note",
        description: "Store a key/value note in working memory. Use this to track state across iterations — counters, lists of results, config values, anything you need to remember. Notes with higher importance survive context pruning. At 20 notes the lowest-importance note is evicted.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "key",
                type_name: "string",
                description: "Short identifier for this note, e.g. 'jobs_applied', 'target_role', 'failed_companies'",
                required: true,
            },
            ParameterSchema {
                name: "value",
                type_name: "string",
                description: "The value to store. Can be any JSON: string, number, array, object.",
                required: true,
            },
            ParameterSchema {
                name: "importance",
                type_name: "integer",
                description: "Importance from 1 (ephemeral, evicted first) to 10 (critical, never evict). Use 10 for goal-critical counters, 1 for debug/temp values.",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "task::get_note",
        description: "Retrieve a note from working memory by key.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "key",
                type_name: "string",
                description: "The key of the note to retrieve",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "task::remove_note",
        description: "Remove a note from working memory by key.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "key",
                type_name: "string",
                description: "The key of the note to remove",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "task::increment_note",
        description: "Increment a numeric note by a delta (positive or negative). Creates the note if it doesn't exist, starting from 0. Useful for counters.",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "key",
                type_name: "string",
                description: "The key of the numeric note",
                required: true,
            },
            ParameterSchema {
                name: "delta",
                type_name: "integer",
                description: "Amount to add (use negative to subtract)",
                required: true,
            },
            ParameterSchema {
                name: "importance",
                type_name: "integer",
                description: "Importance 1-10 (applied on creation; ignored on update)",
                required: true,
            },
        ],
    },
]);

pub fn handle_task_tool(task: &mut TaskState, tool_name: &str, args: &Value) -> Result<String> {
    match tool_name {
        "task::set_agent_goal" => {
            let goal = args["goal"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'goal' parameter"))?
                .to_string();
            task.set_agent_goal(goal.clone());
            Ok(format!("Agent goal set: {}", goal))
        }

        "task::set_plan" => {
            let steps_array = args["steps"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("'steps' must be an array"))?;
            let steps: Vec<String> = steps_array
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if steps.is_empty() {
                return Err(anyhow::anyhow!("Plan must have at least one step"));
            }
            task.set_plan(steps.clone());
            Ok(format!("Plan set with {} steps", steps.len()))
        }

        "task::set_iterations" => {
            let total = args["total"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'total' parameter"))?;
            if total == 0 {
                return Err(anyhow::anyhow!("total must be greater than 0"));
            }
            task.set_iterations(total);
            Ok(format!("Iteration target set: {}", total))
        }

        "task::complete_iteration" => {
            task.complete_iteration();
            let remaining = task.remaining_iterations();
            match remaining {
                Some(r) if r > 0 => Ok(format!(
                    "Iteration complete. Progress: {}/{} ({} remaining)",
                    task.completed_iterations,
                    task.total_iterations.unwrap_or(0),
                    r
                )),
                Some(_) => Ok(format!(
                    "Iteration complete. All {} iterations done.",
                    task.total_iterations.unwrap_or(0)
                )),
                None => Ok(format!("Iteration complete. Total so far: {}", task.completed_iterations)),
            }
        }

        "task::set_current_step" => {
            let step = args["step"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'step' parameter"))?
                .to_string();
            task.set_current_step(&step);
            Ok(format!("Now working on: {}", step))
        }

        "task::mark_step_complete" => {
            task.mark_step_complete();
            Ok("Current step marked complete".to_string())
        }

        "task::mark_complete" => {
            task.mark_complete();
            Ok("Task marked complete".to_string())
        }

        "task::set_note" => {
            let key = args["key"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'key' parameter"))?
                .to_string();
            let importance = args["importance"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'importance' parameter"))? as u8;

            // Accept value as raw JSON — parse if it looks like JSON, otherwise treat as string
            let value = if let Some(s) = args["value"].as_str() {
                serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_string()))
            } else {
                args["value"].clone()
            };

            task.set_note(key.clone(), value.clone(), importance);
            Ok(format!("Note set: [{}] = {} (importance={})", key, value, importance))
        }

        "task::get_note" => {
            let key = args["key"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'key' parameter"))?;
            match task.get_note(key) {
                Some(note) => Ok(format!(
                    "[{}] = {} (importance={})",
                    note.key, note.value, note.importance
                )),
                None => Ok(format!("No note found for key '{}'", key)),
            }
        }

        "task::remove_note" => {
            let key = args["key"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'key' parameter"))?;
            task.remove_note(key);
            Ok(format!("Note removed: '{}'", key))
        }

        "task::increment_note" => {
            let key = args["key"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'key' parameter"))?;
            let delta = args["delta"]
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'delta' parameter"))?;
            let importance = args["importance"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'importance' parameter"))? as u8;

            let new_val = task.increment_note(key, delta, importance)?;
            Ok(format!("[{}] = {} (delta: {:+})", key, new_val, delta))
        }

        _ => Err(anyhow::anyhow!("Unknown task tool: {}", tool_name)),
    }
}

pub fn is_task_tool(tool_name: &str) -> bool {
    tool_name.starts_with("task::")
}
