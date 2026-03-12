use std::sync::Arc;
use crate::pool::GpuHandle;
use crate::api::events::EventSender;
use artificer_shared::db::Db;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use anyhow::Result;

mod specialist;
pub use specialist::SpecialistExecution;

// ============================================================================
// EXECUTION CONTEXT
// ============================================================================

/// Everything an agent needs to execute — request-scoped plumbing.
/// This is NOT visible to the model. It's the execution environment.
pub struct ExecutionContext {
    pub device_id: u64,
    pub device_key: String,
    pub conversation_id: u64,
    pub parent_task_id: Option<u64>,
    pub gpu: GpuHandle,
    pub events: Option<EventSender>,
    pub db: Arc<Db>,
}

// ============================================================================
// TASK PHASE & STEP TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskPhase {
    Planning,
    Executing,
    Reviewing,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    Pending,
    InProgress,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub description: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub key: String,
    pub value: Value,
    pub importance: u8,
}

// ============================================================================
// TASK STATE
// ============================================================================

const MAX_NOTES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: u64,
    pub parent_task_id: Option<u64>,
    pub user_goal: String,
    pub agent_goal: Option<String>,
    pub plan: Option<Vec<PlanStep>>,
    pub phase: TaskPhase,
    pub total_iterations: Option<u64>,
    pub completed_iterations: u64,
    pub notes: Vec<Note>,
    #[serde(skip)]
    pub dirty: bool,
}

impl TaskState {
    pub fn new(id: u64, parent_task_id: Option<u64>, goal: &str) -> Self {
        Self {
            id,
            parent_task_id,
            user_goal: goal.to_string(),
            agent_goal: None,
            plan: None,
            phase: TaskPhase::Planning,
            total_iterations: None,
            completed_iterations: 0,
            notes: Vec::new(),
            dirty: false,
        }
    }

    // -------------------------------------------------------------------------
    // XML rendering
    // -------------------------------------------------------------------------

    pub fn build_task_xml(&self) -> String {
        let mut xml = String::new();
        xml.push_str("<task>\n");
        xml.push_str(&format!("  <user_goal>{}</user_goal>\n", self.user_goal));

        if let Some(ref goal) = self.agent_goal {
            xml.push_str(&format!("  <agent_goal>{}</agent_goal>\n", goal));
        }

        let phase_str = match self.phase {
            TaskPhase::Planning => "planning",
            TaskPhase::Executing => "executing",
            TaskPhase::Reviewing => "reviewing",
            TaskPhase::Complete => "complete",
            TaskPhase::Failed => "failed",
        };
        xml.push_str(&format!("  <phase>{}</phase>\n", phase_str));

        if let Some(ref plan) = self.plan {
            xml.push_str("  <plan>\n");
            for step in plan {
                let status_str = match step.status {
                    StepStatus::Pending => "pending",
                    StepStatus::InProgress => "in_progress",
                    StepStatus::Complete => "complete",
                    StepStatus::Failed => "failed",
                };
                if step.status == StepStatus::InProgress {
                    if let Some(total) = self.total_iterations {
                        xml.push_str(&format!(
                            "    <step status=\"{}\" progress=\"{}/{}\">{}</step>\n",
                            status_str, self.completed_iterations, total, step.description
                        ));
                        continue;
                    }
                }
                xml.push_str(&format!(
                    "    <step status=\"{}\">{}</step>\n",
                    status_str, step.description
                ));
            }
            xml.push_str("  </plan>\n");
        }

        if let Some(total) = self.total_iterations {
            xml.push_str(&format!(
                "  <iterations completed=\"{}\" total=\"{}\"/>\n",
                self.completed_iterations, total
            ));
        }

        if !self.notes.is_empty() {
            xml.push_str("  <working_memory>\n");
            let mut sorted = self.notes.clone();
            sorted.sort_by(|a, b| b.importance.cmp(&a.importance));
            for note in &sorted {
                xml.push_str(&format!(
                    "    <note key=\"{}\" importance=\"{}\">{}</note>\n",
                    note.key, note.importance, note.value
                ));
            }
            xml.push_str("  </working_memory>\n");
        }

        xml.push_str("</task>");
        xml
    }

    // -------------------------------------------------------------------------
    // Mutation methods (in-memory only — set dirty flag)
    // -------------------------------------------------------------------------

    pub fn set_agent_goal(&mut self, goal: String) {
        self.agent_goal = Some(goal);
        self.dirty = true;
    }

    pub fn set_plan(&mut self, steps: Vec<String>) {
        self.plan = Some(
            steps.into_iter()
                .map(|description| PlanStep {
                    description,
                    status: StepStatus::Pending,
                })
                .collect(),
        );
        self.phase = TaskPhase::Executing;
        self.dirty = true;
    }

    /// Find the step in plan matching description and set it to InProgress.
    /// If no match, does nothing.
    pub fn set_current_step(&mut self, step_desc: &str) {
        if let Some(ref mut plan) = self.plan {
            if let Some(step) = plan.iter_mut().find(|s| s.description == step_desc) {
                step.status = StepStatus::InProgress;
                self.dirty = true;
            }
        }
    }

    /// Find first InProgress step and set it to Complete.
    pub fn mark_step_complete(&mut self) {
        if let Some(ref mut plan) = self.plan {
            if let Some(step) = plan.iter_mut().find(|s| s.status == StepStatus::InProgress) {
                step.status = StepStatus::Complete;
                self.dirty = true;
            }
        }
    }

    pub fn mark_complete(&mut self) {
        self.phase = TaskPhase::Complete;
        if let Some(ref mut plan) = self.plan {
            if let Some(step) = plan.iter_mut().find(|s| s.status == StepStatus::InProgress) {
                step.status = StepStatus::Complete;
            }
        }
        self.dirty = true;
    }

    pub fn mark_failed(&mut self) {
        self.phase = TaskPhase::Failed;
        self.dirty = true;
    }

    pub fn set_iterations(&mut self, total: u64) {
        self.total_iterations = Some(total);
        self.dirty = true;
    }

    pub fn complete_iteration(&mut self) {
        self.completed_iterations += 1;
        self.dirty = true;
    }

    /// Set a note by key. Updates in place if key exists. Evicts lowest-importance
    /// note if at capacity. importance is clamped 1–10.
    pub fn set_note(&mut self, key: String, value: Value, importance: u8) {
        let importance = importance.clamp(1, 10);

        if let Some(existing) = self.notes.iter_mut().find(|n| n.key == key) {
            existing.value = value;
            existing.importance = importance;
            self.dirty = true;
            return;
        }

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

        self.notes.push(Note { key, value, importance });
        self.dirty = true;
    }

    pub fn get_note(&self, key: &str) -> Option<&Note> {
        self.notes.iter().find(|n| n.key == key)
    }

    pub fn remove_note(&mut self, key: &str) {
        self.notes.retain(|n| n.key != key);
        self.dirty = true;
    }

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
    // Status queries
    // -------------------------------------------------------------------------

    pub fn is_complete(&self) -> bool {
        self.phase == TaskPhase::Complete
    }

    pub fn is_failed(&self) -> bool {
        self.phase == TaskPhase::Failed
    }

    pub fn has_remaining_iterations(&self) -> bool {
        match self.total_iterations {
            Some(total) => self.completed_iterations < total,
            None => false,
        }
    }

    pub fn remaining_iterations(&self) -> Option<u64> {
        self.total_iterations
            .map(|total| total.saturating_sub(self.completed_iterations))
    }

    // -------------------------------------------------------------------------
    // Persistence
    // -------------------------------------------------------------------------

    pub fn persist(&self, ctx: &ExecutionContext) -> Result<()> {
        let plan_json = self.plan.as_ref()
            .map(|p| serde_json::to_string(p))
            .transpose()?;

        let working_memory_json = serde_json::json!({
            "agent_goal": self.agent_goal,
            "phase": self.phase,
            "total_iterations": self.total_iterations,
            "completed_iterations": self.completed_iterations,
            "notes": self.notes,
        })
        .to_string();

        ctx.db.checkpoint_task(
            self.id as i64,
            plan_json.as_deref(),
            Some(&working_memory_json),
        )
    }

    pub fn persist_complete(&self, ctx: &ExecutionContext) -> Result<()> {
        ctx.db.complete_task(self.id as i64)
    }

    pub fn persist_failed(&self, ctx: &ExecutionContext) -> Result<()> {
        ctx.db.fail_task(self.id as i64)
    }

    pub fn persist_if_dirty(&mut self, ctx: &ExecutionContext) -> Result<()> {
        if self.dirty {
            self.persist(ctx)?;
            self.dirty = false;
        }
        Ok(())
    }
}

// ============================================================================
// AGENT STATE TRAIT
// ============================================================================

pub trait AgentState {
    /// Build the complete XML state string for the user message.
    fn build_state_xml(&self) -> String;

    /// Should the execution loop terminate?
    fn should_terminate(&self) -> bool;

    /// Build the final output returned to the caller.
    fn build_response(&self) -> String;
}
