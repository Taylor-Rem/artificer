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
pub struct TaskStep {
    pub goal: String,
    pub progress: TaskStatus,
}

/// A note stored in the task's working memory.
/// Notes with higher importance survive context pruning longer.
/// Capped at 20 notes total — lowest importance notes are evicted first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNote {
    /// Short identifier for retrieval and updates (e.g. "jobs_applied_count")
    pub key: String,
    /// The value — any JSON: string, number, array, object, bool
    pub value: Value,
    /// Importance from 1 (ephemeral) to 10 (critical). Used for eviction priority.
    pub importance: u8,
}

const MAX_NOTES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub parent_task_id: Option<u64>,
    pub user_goal: String,
    pub agent_goal: Option<String>,
    pub plan: Option<Vec<TaskStep>>,
    pub current_step: Option<TaskStep>,
    pub progress: TaskStatus,

    /// Total number of iterations this task expects to run.
    /// Set once at planning time. None means the task is not iteration-based.
    pub total_iterations: Option<u64>,

    /// How many iterations have been completed so far.
    pub completed_iterations: u64,

    /// Structured working memory: arbitrary key/value notes with importance scores.
    /// Capped at MAX_NOTES (20). Lowest-importance notes evicted when cap is reached.
    pub notes: Vec<TaskNote>,

    /// Non-serialized execution context
    #[serde(skip)]
    pub(crate) db: Option<Arc<Db>>,
    #[serde(skip)]
    conversation_id: u64,
    #[serde(skip)]
    gpu: Option<GpuHandle>,
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
            total_iterations: None,
            completed_iterations: 0,
            notes: Vec::new(),
            db: Some(db),
            conversation_id: context.conversation_id,
            gpu: Some(context.gpu.clone()),
        }
    }

    pub fn conversation_id(&self) -> u64 {
        self.conversation_id
    }

    pub fn gpu(&self) -> &GpuHandle {
        self.gpu.as_ref().expect("GPU not set in task")
    }

    /// Generate a compact summary of current task state for the system prompt.
    /// Sorted by importance descending so the most critical notes appear first.
    pub fn state_summary(&self) -> String {
        let mut parts = vec![
            format!("Task ID: {}", self.id),
            format!("User Goal: {}", self.user_goal),
        ];

        if let Some(ref agent_goal) = self.agent_goal {
            parts.push(format!("Agent Goal: {}", agent_goal));
        }

        parts.push(format!("Status: {:?}", self.progress));

        // Iteration progress
        if let Some(total) = self.total_iterations {
            parts.push(format!(
                "Progress: {}/{} iterations complete",
                self.completed_iterations, total
            ));
        }

        if let Some(ref plan) = self.plan {
            parts.push("Plan:".to_string());
            for (i, step) in plan.iter().enumerate() {
                parts.push(format!("  {}. {} ({:?})", i + 1, step.goal, step.progress));
            }
        }

        if let Some(ref current) = self.current_step {
            parts.push(format!(
                "Current Step: {} ({:?})",
                current.goal, current.progress
            ));
        }

        // Notes sorted by importance descending
        if !self.notes.is_empty() {
            parts.push("Working Memory:".to_string());
            let mut sorted_notes = self.notes.clone();
            sorted_notes.sort_by(|a, b| b.importance.cmp(&a.importance));
            for note in &sorted_notes {
                parts.push(format!(
                    "  [{}] (importance={}) {}",
                    note.key, note.importance, note.value
                ));
            }
        }

        parts.join("\n")
    }

    /// Generate XML summary of task state for the specialist's message 3.
    pub fn state_summary_xml(&self) -> String {
        let mut xml = String::new();
        xml.push_str("<task_progress>\n");
        xml.push_str(&format!("  <user_goal>{}</user_goal>\n", self.user_goal));

        if let Some(ref goal) = self.agent_goal {
            xml.push_str(&format!("  <agent_goal>{}</agent_goal>\n", goal));
        }

        if let Some(ref plan) = self.plan {
            xml.push_str("  <plan>\n");
            for step in plan.iter() {
                xml.push_str("    <task_step>\n");
                xml.push_str(&format!("      <goal>{}</goal>\n", step.goal));
                xml.push_str(&format!("      <progress>{:?}</progress>\n", step.progress));
                xml.push_str("    </task_step>\n");
            }
            xml.push_str("  </plan>\n");
        }

        if let Some(total) = self.total_iterations {
            xml.push_str(&format!(
                "  <iterations>{}/{}</iterations>\n",
                self.completed_iterations, total
            ));
        }

        if let Some(ref step) = self.current_step {
            xml.push_str(&format!("  <current_step>{}</current_step>\n", step.goal));
        }

        xml.push_str("</task_progress>\n");

        if !self.notes.is_empty() {
            xml.push_str("\n<notes>\n");
            let mut sorted = self.notes.clone();
            sorted.sort_by(|a, b| b.importance.cmp(&a.importance));
            for note in &sorted {
                xml.push_str(&format!(
                    "  <note key=\"{}\" importance=\"{}\">{}</note>\n",
                    note.key, note.importance, note.value
                ));
            }
            xml.push_str("</notes>\n");
        }

        xml
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

    pub fn set_iterations(&mut self, total: u64) {
        self.total_iterations = Some(total);
        let _ = self.persist();
    }

    pub fn complete_iteration(&mut self) {
        self.completed_iterations += 1;
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
    // Working memory (notes)
    // -------------------------------------------------------------------------

    /// Set a note by key. If the key already exists, the value and importance
    /// are updated in place. If adding a new note would exceed MAX_NOTES,
    /// the existing note with the lowest importance is evicted first.
    ///
    /// importance: 1 (ephemeral, evicted first) to 10 (critical, survives pruning)
    pub fn set_note(&mut self, key: String, value: Value, importance: u8) {
        let importance = importance.clamp(1, 10);

        // Update existing note if key matches
        if let Some(existing) = self.notes.iter_mut().find(|n| n.key == key) {
            existing.value = value;
            existing.importance = importance;
            let _ = self.persist();
            return;
        }

        // Evict lowest-importance note if at capacity
        if self.notes.len() >= MAX_NOTES {
            if let Some(min_pos) = self
                .notes
                .iter()
                .enumerate()
                .min_by_key(|(_, n)| n.importance)
                .map(|(i, _)| i)
            {
                self.notes.remove(min_pos);
            }
        }

        self.notes.push(TaskNote { key, value, importance });
        let _ = self.persist();
    }

    /// Get a note by key. Returns None if not found.
    pub fn get_note(&self, key: &str) -> Option<&TaskNote> {
        self.notes.iter().find(|n| n.key == key)
    }

    /// Remove a note by key.
    pub fn remove_note(&mut self, key: &str) {
        self.notes.retain(|n| n.key != key);
        let _ = self.persist();
    }

    /// Increment a numeric note by delta. Creates the note if it doesn't exist.
    /// The note must hold a JSON number; returns an error string if it holds
    /// a non-numeric value.
    pub fn increment_note(&mut self, key: &str, delta: i64, importance: u8) -> Result<i64> {
        let current = self
            .notes
            .iter()
            .find(|n| n.key == key)
            .and_then(|n| n.value.as_i64())
            .unwrap_or(0);

        let next = current + delta;
        self.set_note(key.to_string(), Value::Number(next.into()), importance);
        Ok(next)
    }

    // -------------------------------------------------------------------------
    // Parent task read methods (specialist sub-tasks only)
    // -------------------------------------------------------------------------

    pub fn get_parent_goal(&self) -> Result<String> {
        let parent_id = self.parent_task_id
            .ok_or_else(|| anyhow::anyhow!("No parent task — this is a primary orchestrator task"))?;

        let db = self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?;

        let (goal, _plan) = db.get_task_info(parent_id)?
            .ok_or_else(|| anyhow::anyhow!("Parent task {} not found", parent_id))?;

        Ok(goal)
    }

    pub fn get_parent_plan(&self) -> Result<Option<String>> {
        let parent_id = self.parent_task_id
            .ok_or_else(|| anyhow::anyhow!("No parent task — this is a primary orchestrator task"))?;

        let db = self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?;

        let (_goal, plan) = db.get_task_info(parent_id)?
            .ok_or_else(|| anyhow::anyhow!("Parent task {} not found", parent_id))?;

        Ok(plan)
    }

    pub fn is_specialist_task(&self) -> bool {
        self.parent_task_id.is_some()
    }

    pub fn is_primary_task(&self) -> bool {
        self.parent_task_id.is_none()
    }

    // -------------------------------------------------------------------------
    // Persistence helpers
    // -------------------------------------------------------------------------

    pub fn persist(&self) -> Result<()> {
        let db = self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?;

        let plan_json = self.plan.as_ref()
            .map(|p| serde_json::to_string(p))
            .transpose()?;

        let working_memory_json = serde_json::json!({
            "agent_goal": self.agent_goal,
            "current_step": self.current_step,
            "total_iterations": self.total_iterations,
            "completed_iterations": self.completed_iterations,
            "notes": self.notes,
        }).to_string();

        db.checkpoint_task(
            self.id as i64,
            plan_json.as_deref(),
            Some(&working_memory_json),
        )
    }

    pub fn persist_complete(&self) -> Result<()> {
        self.db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database reference not available"))?
            .complete_task(self.id as i64)
    }

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

    /// True if an iteration-based task still has work remaining.
    pub fn has_remaining_iterations(&self) -> bool {
        match self.total_iterations {
            Some(total) => self.completed_iterations < total,
            None => false,
        }
    }

    /// Remaining iterations, or None if not iteration-based.
    pub fn remaining_iterations(&self) -> Option<u64> {
        self.total_iterations
            .map(|total| total.saturating_sub(self.completed_iterations))
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
    ToolSchema {
        name: "task::get_parent_goal",
        description: "Get the goal of the parent task. Only available for specialist sub-tasks.",
        location: ToolLocation::Server,
        parameters: vec![],
    },
    ToolSchema {
        name: "task::get_parent_plan",
        description: "Get the plan of the parent task. Only available for specialist sub-tasks.",
        location: ToolLocation::Server,
        parameters: vec![],
    },
]);

// ============================================================================
// TASK TOOL HANDLERS
// ============================================================================

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

#[allow(dead_code)]
pub fn is_task_tool(tool_name: &str) -> bool {
    tool_name.starts_with("task::")
}