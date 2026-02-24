use anyhow::Result;
use reqwest::Client;
use serde_json::Value;

use artificer_shared::db::Db;
use crate::api::events::EventSender;
use crate::pool::GpuHandle;
use crate::specialist::SPECIALISTS;
use crate::Message;

use super::task::Task;

/// Dispatch a tool call from the Orchestrator's loop.
/// Working memory tools are handled locally.
/// Delegation tools run a specialist and return its result.
pub async fn handle(
    tool_name: &str,
    args: &Value,
    task: &mut Task,
    db: &Db,
    gpu: &GpuHandle,
    events: Option<&EventSender>,
    client: &Client,
) -> Result<String> {
    match tool_name {
        // ---- Working memory ----

        "working_memory::set_plan" => {
            let steps: Vec<String> = args["steps"]
                .as_array()
                .map(|arr| arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect())
                .unwrap_or_default();
            let count = steps.len();
            task.plan = steps;
            // Persist immediately — don't wait for an explicit checkpoint
            persist(task, db)?;
            Ok(format!("Plan set with {} steps.", count))
        }

        "working_memory::set_current_step" => {
            let step = args["step"].as_str().unwrap_or("").to_string();
            task.current_step = Some(step.clone());
            persist(task, db)?;
            Ok(format!("Now working on: {}", step))
        }

        "working_memory::set_state" => {
            let key = args["key"].as_str().unwrap_or("").to_string();
            let value = args["value"].as_str().unwrap_or("").to_string();
            task.remember(key.clone(), value.clone());
            persist(task, db)?;
            Ok(format!("Saved: {} = {}", key, value))
        }

        "working_memory::get_state" => {
            let key = args["key"].as_str().unwrap_or("");
            match task.recall(key) {
                Some(val) => Ok(val.clone()),
                None => Ok(format!("No value stored for '{}'.", key)),
            }
        }

        "working_memory::checkpoint" => {
            let summary = args["summary"].as_str().unwrap_or("").to_string();
            let progress = args["progress"].as_str().unwrap_or("").to_string();
            task.record_progress(
                task.current_step.clone().unwrap_or_else(|| summary.clone()),
                summary.clone(),
            );
            task.remember("progress".to_string(), progress.clone());
            persist(task, db)?;
            Ok(format!("Checkpoint saved. Progress: {}", progress))
        }

        "working_memory::mark_complete" => {
            let summary = args["summary"].as_str().unwrap_or("Task complete").to_string();
            task.record_progress(
                task.current_step.clone().unwrap_or_else(|| "Final step".to_string()),
                summary.clone(),
            );
            task.complete = true;
            persist(task, db)?;
            Ok(format!("Task marked complete: {}", summary))
        }

        // ---- Specialist delegation ----

        "delegate::web_research" => {
            let prompt = args["prompt"].as_str().unwrap_or("").to_string();
            delegate("web_research", prompt, gpu, events, client).await
        }

        "delegate::file_smith" => {
            let prompt = args["prompt"].as_str().unwrap_or("").to_string();
            delegate("file_smith", prompt, gpu, events, client).await
        }

        // ---- Unknown ----

        unknown => Ok(format!(
            "Unknown tool '{}'. Check the tool list and try again.",
            unknown
        )),
    }
}

/// Run a specialist to completion and return its synthesized result.
/// The specialist gets its own agentic loop — the Orchestrator just sees the output.
async fn delegate(
    specialist_name: &str,
    prompt: String,
    gpu: &GpuHandle,
    events: Option<&EventSender>,
    client: &Client,
) -> Result<String> {
    let specialist = SPECIALISTS.iter()
        .find(|s| s.name == specialist_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown specialist: {}", specialist_name))?;

    if let Some(ev) = events {
        ev.task_switch("orchestrator", specialist_name);
    }

    let messages = vec![Message {
        role: "user".to_string(),
        content: Some(prompt),
        tool_calls: None,
    }];

    let response = specialist.execute(gpu, messages, events, client).await?;

    if let Some(ev) = events {
        ev.task_switch(specialist_name, "orchestrator");
    }

    Ok(response.content.unwrap_or_default())
}

/// Persist current task working memory and plan to the DB.
/// Called after every mutation so a crash or context prune never loses state.
fn persist(task: &Task, db: &Db) -> Result<()> {
    let plan_json = serde_json::to_string(&task.plan)?;
    let memory_json = serde_json::to_string(&task.working_memory)?;
    db.checkpoint_task(task.id, Some(&plan_json), Some(&memory_json))?;
    Ok(())
}

/// The Orchestrator's tool schema, passed to the model on every call.
/// Written by hand — the Orchestrator is not a specialist and doesn't use the macro system.
pub fn definitions() -> Vec<Value> {
    vec![
        // ---- Delegation ----
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "delegate::web_research",
                "description": "Delegate a web research task to the WebResearch specialist. \
                    The specialist will search, fetch pages, and return a synthesized result. \
                    Be specific: tell it exactly what to find and what you need extracted.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "Specific instructions for the specialist."
                        }
                    },
                    "required": ["prompt"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "delegate::file_smith",
                "description": "Delegate a file system task to the FileSmith specialist. \
                    The specialist can read, write, list, and manipulate files. \
                    Be specific about paths, operations, and what to return.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "Specific instructions for the specialist."
                        }
                    },
                    "required": ["prompt"]
                }
            }
        }),

        // ---- Working memory ----
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "working_memory::set_plan",
                "description": "Set your plan for this task as an ordered list of steps. \
                    Call early before executing. Revise as you learn more.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "steps": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Ordered steps to complete the task."
                        }
                    },
                    "required": ["steps"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "working_memory::set_current_step",
                "description": "Record which step you are currently executing.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "step": { "type": "string", "description": "The current step." }
                    },
                    "required": ["step"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "working_memory::set_state",
                "description": "Save a key-value pair to working memory. \
                    Use for counters, current targets, accumulated results, anything \
                    you need to remember across steps.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string" },
                        "value": { "type": "string" }
                    },
                    "required": ["key", "value"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "working_memory::get_state",
                "description": "Retrieve a value from working memory by key.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string" }
                    },
                    "required": ["key"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "working_memory::checkpoint",
                "description": "Save a checkpoint after completing a significant chunk of work. \
                    Records progress and keeps context clean for long tasks. \
                    Call after every major step.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "summary": {
                            "type": "string",
                            "description": "What was just completed and what was learned."
                        },
                        "progress": {
                            "type": "string",
                            "description": "Overall progress toward the goal (e.g. '3 of 10 files reviewed')."
                        }
                    },
                    "required": ["summary", "progress"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "working_memory::mark_complete",
                "description": "Mark the task as fully complete. \
                    Only call this when the goal is genuinely achieved. \
                    After calling this, write your final response to the user.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "summary": {
                            "type": "string",
                            "description": "Brief summary of what was accomplished."
                        }
                    },
                    "required": ["summary"]
                }
            }
        }),
    ]
}