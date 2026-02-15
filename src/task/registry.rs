// src/task/registry.rs
use once_cell::sync::Lazy;
use crate::tools::{Tool, ToolSchema, ParameterSchema};
use super::Task;

static TASK_SCHEMAS: Lazy<Vec<ToolSchema>> = Lazy::new(|| {
    vec![
        ToolSchema {
            name: "switch_task",
            description: "Switch to a different task type based on user needs",
            parameters: vec![
                ParameterSchema {
                    name: "task",
                    type_name: "string",
                    description: "Task to switch to: 'chat', 'research', 'code_review'",
                    required: true,
                }
            ],
        }
    ]
});

/// Get tasks available to a specific parent task
pub fn get_available_tasks(current_task: &Task) -> Vec<Tool> {
    match current_task {
        Task::Chat => {
            // Chat can switch to anything
            vec!["research", "code_review"]
        }
        Task::Research => {
            // Research might want specialized sub-tasks
            vec!["web_search", "summarize_sources", "chat"]
        }
        Task::CodeReview => {
            // Code review goes back to chat
            vec!["chat"]
        }
        _ => vec![] // Background tasks don't switch
    }
        .into_iter()
        .filter_map(|name| {
            TASK_SCHEMAS.iter().find(|s| {
                // Filter schema to only include allowed tasks
                s.name == "switch_task"
            }).cloned()
        })
        .map(|s| s.to_tool())
        .collect()
}

/// Handle task switching tool call
pub fn switch_task(task_name: &str) -> anyhow::Result<Task> {
    Task::from_str(task_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown task: {}", task_name))
}