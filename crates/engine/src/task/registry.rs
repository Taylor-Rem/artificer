use once_cell::sync::Lazy;
use artificer_tools::{Tool, ToolSchema, ParameterSchema, ToolLocation};
use super::Task;

static TASK_SCHEMA: Lazy<ToolSchema> = Lazy::new(|| {
    ToolSchema {
        name: "switch_task",
        description: "Switch to a different task type based on user needs",
        location: ToolLocation::Server,
        parameters: vec![
            ParameterSchema {
                name: "task",
                type_name: "string",
                description: "Task to switch to",
                required: true,
            }
        ],
    }
});

/// Get tasks available to a specific parent task
pub fn get_available_tasks(current_task: &Task) -> Vec<Tool> {
    let available = current_task.available_switches();

    if available.is_empty() {
        return vec![];
    }

    // Build description with available tasks
    let task_list = available.iter()
        .map(|t| format!("'{}' ({})", t.title(), t.description()))
        .collect::<Vec<_>>()
        .join(", ");

    let mut schema = TASK_SCHEMA.clone();
    schema.parameters[0].description = Box::leak(format!("Task to switch to. Available: {}", task_list).into_boxed_str());

    vec![schema.to_tool()]
}

/// Handle task switching tool call
pub fn switch_task(task_name: &str) -> anyhow::Result<Task> {
    Task::from_str(task_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown task: {}", task_name))
}
