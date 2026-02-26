use std::sync::Arc;
use crate::agent::AgentContext;
use crate::pool::GpuHandle;
use artificer_shared::db::Db;
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
    pub parent_task_id: Option<u64>,
    pub user_goal: String,
    pub agent_goal: Option<String>,
    pub plan: Option<Vec<TaskStep>>,
    pub current_step: Option<TaskStep>,
    pub progress: TaskStatus,

    /// Non-serialized fields for execution context
    #[serde(skip)]
    pub(crate) db: Option<Arc<Db>>,
    #[serde(skip)]
    conversation_id: u64,
    #[serde(skip)]
    gpu: Option<GpuHandle>,
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
    pub fn new(
        context: &AgentContext,
        parent_task_id: Option<u64>,
        goal: &str,
        db: Arc<Db>,
    ) -> Self {
        let task_id = db
            .create_task(context.device_id, context.conversation_id, parent_task_id, goal)
            .expect("Failed to create task");

        Self {
            id: task_id,
            parent_task_id,
            user_goal: goal.to_string(),
            agent_goal: None,
            plan: None,
            current_step: None,
            progress: TaskStatus::NotStarted,
            db: Some(db),
            conversation_id: context.conversation_id,
            gpu: Some(context.gpu.clone()),
        }
    }

    /// Get the conversation this task belongs to
    pub fn conversation_id(&self) -> u64 {
        self.conversation_id
    }

    /// Get the GPU handle for this task
    pub fn gpu(&self) -> &GpuHandle {
        self.gpu.as_ref().expect("GPU not set in task")
    }

    /// Generate a compact summary of current task state for the system prompt.
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

    // -------------------------------------------------------------------------
    // Modification methods (own task only — auto-persist)
    // -------------------------------------------------------------------------

    pub fn set_agent_goal(&mut self, goal: String) {
        self.agent_goal = Some(goal);
        let _ = self.persist();
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
        let _ = self.persist();
    }

    pub fn set_current_step(&mut self, step_goal: String) {
        self.current_step = Some(TaskStep {
            goal: step_goal,
            progress: TaskStatus::InProgress,
        });
        let _ = self.persist();
    }

    pub fn mark_step_complete(&mut self) {
        if let Some(ref mut step) = self.current_step {
            step.progress = TaskStatus::Completed;
        }
        let _ = self.persist();
    }

    pub fn mark_complete(&mut self) {
        self.progress = TaskStatus::Completed;
        if let Some(ref mut step) = self.current_step {
            step.progress = TaskStatus::Completed;
        }
        let _ = self.persist_complete();
    }

    pub fn mark_failed(&mut self, _reason: Option<String>) {
        self.progress = TaskStatus::Failed;
        let _ = self.persist_failed();
    }

    // -------------------------------------------------------------------------
    // Parent task read methods (specialist sub-tasks only)
    // -------------------------------------------------------------------------

    /// Get the parent task's goal. Returns error if this is a primary task.
    pub fn get_parent_goal(&self) -> Result<String> {
        let parent_id = self.parent_task_id
            .ok_or_else(|| anyhow::anyhow!("No parent task — this is a primary orchestrator task"))?;

        let db = self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?;

        let (goal, _plan) = db.get_task_info(parent_id)?
            .ok_or_else(|| anyhow::anyhow!("Parent task {} not found", parent_id))?;

        Ok(goal)
    }

    /// Get the parent task's plan. Returns error if this is a primary task.
    pub fn get_parent_plan(&self) -> Result<Option<String>> {
        let parent_id = self.parent_task_id
            .ok_or_else(|| anyhow::anyhow!("No parent task — this is a primary orchestrator task"))?;

        let db = self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?;

        let (_goal, plan) = db.get_task_info(parent_id)?
            .ok_or_else(|| anyhow::anyhow!("Parent task {} not found", parent_id))?;

        Ok(plan)
    }

    /// True if this task is a specialist sub-task (has a parent).
    pub fn is_specialist_task(&self) -> bool {
        self.parent_task_id.is_some()
    }

    /// True if this task is a primary orchestrator task (no parent).
    pub fn is_primary_task(&self) -> bool {
        self.parent_task_id.is_none()
    }

    // -------------------------------------------------------------------------
    // Persistence helpers
    // -------------------------------------------------------------------------

    /// Persist current in-memory state to the database.
    pub fn persist(&self) -> Result<()> {
        let db = self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?;

        let plan_json = self.plan.as_ref()
            .map(|p| serde_json::to_string(p))
            .transpose()?;

        let working_memory_json = serde_json::json!({
            "agent_goal": self.agent_goal,
            "current_step": self.current_step,
        }).to_string();

        db.checkpoint_task(
            self.id as i64,
            plan_json.as_deref(),
            Some(&working_memory_json),
        )
    }

    /// Mark task complete in database.
    pub fn persist_complete(&self) -> Result<()> {
        self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?
            .complete_task(self.id as i64)
    }

    /// Mark task failed in database.
    pub fn persist_failed(&self) -> Result<()> {
        self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?
            .fail_task(self.id as i64)
    }

    // -------------------------------------------------------------------------
    // Status query helpers
    // -------------------------------------------------------------------------

    pub fn status(&self) -> &TaskStatus {
        &self.progress
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.progress, TaskStatus::Completed)
    }

    pub fn is_in_progress(&self) -> bool {
        matches!(self.progress, TaskStatus::InProgress)
    }

    pub fn has_failed(&self) -> bool {
        matches!(self.progress, TaskStatus::Failed)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn parent_id(&self) -> Option<u64> {
        self.parent_task_id
    }
}

// ============================================================================
// TASK TOOLS DEFINITIONS
// ============================================================================

use once_cell::sync::Lazy;
use artificer_shared::schemas::{ToolSchema, ParameterSchema, ToolLocation};

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
        description: "Set your plan for accomplishing this task as an ordered list of steps.",
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
        description: "Mark the current step as complete and move to the next one.",
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
        name: "task::get_parent_goal",
        description: "Get the goal of the parent task. Only available for specialist sub-tasks. Returns the orchestrator's original goal.",
        location: ToolLocation::Server,
        parameters: vec![],
    },
    ToolSchema {
        name: "task::get_parent_plan",
        description: "Get the plan of the parent task. Only available for specialist sub-tasks. Returns the orchestrator's current plan.",
        location: ToolLocation::Server,
        parameters: vec![],
    },
]);

// ============================================================================
// TASK TOOL HANDLERS
// ============================================================================

/// Handle a task tool call and mutate the task accordingly.
/// Called by AgentExecution in Request 5.
#[allow(dead_code)]
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

        "task::get_parent_goal" => {
            match task.get_parent_goal() {
                Ok(goal) => Ok(format!("Parent task goal: {}", goal)),
                Err(e) => Ok(format!("Error: {}", e)),
            }
        }

        "task::get_parent_plan" => {
            match task.get_parent_plan() {
                Ok(Some(plan)) => Ok(format!("Parent task plan: {}", plan)),
                Ok(None) => Ok("Parent task has no plan set yet".to_string()),
                Err(e) => Ok(format!("Error: {}", e)),
            }
        }

        _ => Err(anyhow::anyhow!("Unknown task tool: {}", tool_name)),
    }
}

/// Check if a tool name is a task management tool.
/// Called by AgentExecution in Request 5.
#[allow(dead_code)]
pub fn is_task_tool(tool_name: &str) -> bool {
    tool_name.starts_with("task::")
}
