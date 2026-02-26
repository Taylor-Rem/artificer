use crate::agent::AgentContext;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use anyhow::Result;

// ============================================================================
// TASK TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    NotStarted,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub user_goal: String,
    pub agent_goal: Option<String>,
    pub plan: Option<Vec<TaskStep>>,
    pub current_step: Option<TaskStep>,
    pub progress: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    pub goal: String,
    pub progress: TaskStatus,
}

// ============================================================================
// TASK IMPLEMENTATION
// ============================================================================

impl Task {
    pub fn new(context: &AgentContext, goal: &str) -> Self {
        let task_id = context.db
            .create_task(context.device_id, context.conversation_id, goal)
            .expect("Failed to create task");

        Self {
            id: task_id,
            user_goal: goal.to_string(),
            agent_goal: None,
            plan: None,
            current_step: None,
            progress: TaskStatus::NotStarted,
        }
    }

    /// Generate a compact summary of current task state for the system prompt
    pub fn state_summary(&self) -> String {
        let mut parts = vec![
            format!("Task ID: {}", self.id),
            format!("User Goal: {}", self.user_goal),
        ];

        if let Some(ref agent_goal) = self.agent_goal {
            parts.push(format!("Agent Goal: {}", agent_goal));
        }

        parts.push(format!("Status: {:?}", self.progress));

        if let Some(ref plan) = self.plan {
            parts.push("Plan:".to_string());
            for (i, step) in plan.iter().enumerate() {
                parts.push(format!("  {}. {} ({:?})", i + 1, step.goal, step.progress));
            }
        }

        if let Some(ref current) = self.current_step {
            parts.push(format!("Current Step: {} ({:?})", current.goal, current.progress));
        }

        parts.join("\n")
    }

    pub fn set_plan(&mut self, steps: Vec<String>) {
        self.plan = Some(
            steps.into_iter()
                .map(|goal| TaskStep {
                    goal,
                    progress: TaskStatus::NotStarted,
                })
                .collect()
        );
        self.progress = TaskStatus::InProgress;
    }

    pub fn set_agent_goal(&mut self, goal: String) {
        self.agent_goal = Some(goal);
    }

    pub fn set_current_step(&mut self, step_goal: String) {
        self.current_step = Some(TaskStep {
            goal: step_goal,
            progress: TaskStatus::InProgress,
        });
    }

    pub fn mark_step_complete(&mut self) {
        if let Some(ref mut step) = self.current_step {
            step.progress = TaskStatus::Completed;
        }
    }

    pub fn mark_complete(&mut self) {
        self.progress = TaskStatus::Completed;
        if let Some(ref mut step) = self.current_step {
            step.progress = TaskStatus::Completed;
        }
    }

    pub fn mark_failed(&mut self, reason: Option<String>) {
        self.progress = TaskStatus::Failed;
        // Optionally store reason somewhere
    }
}

// ============================================================================
// TASK TOOLS DEFINITIONS
// ============================================================================

use artificer_shared::schemas::{ToolSchema, ParameterSchema, ToolLocation};

pub const TASK_TOOLS: &[ToolSchema] = &[
    ToolSchema {
        name: "task::set_agent_goal",
        description: "Set your interpretation of the user's goal. Call this early to clarify your understanding.",
        location: ToolLocation::Server,
        parameters: &[
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
        description: "Set your plan for accomplishing this task as an ordered list of steps.",
        location: ToolLocation::Server,
        parameters: &[
            ParameterSchema {
                name: "steps",
                type_name: "array",
                description: "Ordered list of step descriptions",
                required: true,
            },
        ],
    },
    ToolSchema {
        name: "task::set_current_step",
        description: "Set which step you are currently working on.",
        location: ToolLocation::Server,
        parameters: &[
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
        description: "Mark the current step as complete and move to the next one.",
        location: ToolLocation::Server,
        parameters: &[],
    },
    ToolSchema {
        name: "task::mark_complete",
        description: "Mark the entire task as complete. Only call this when the goal is fully achieved.",
        location: ToolLocation::Server,
        parameters: &[],
    },
];

// ============================================================================
// TASK TOOL HANDLERS
// ============================================================================

/// Handle a task tool call and mutate the task accordingly
pub fn handle_task_tool(task: &mut Task, tool_name: &str, args: &Value) -> Result<String> {
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

        "task::set_current_step" => {
            let step = args["step"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'step' parameter"))?
                .to_string();

            task.set_current_step(step.clone());
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

        _ => Err(anyhow::anyhow!("Unknown task tool: {}", tool_name)),
    }
}

/// Check if a tool name is a task management tool
pub fn is_task_tool(tool_name: &str) -> bool {
    tool_name.starts_with("task::")
}