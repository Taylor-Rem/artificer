use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Runtime state for a single Orchestrator task.
/// Created when a request comes in, updated throughout execution,
/// persisted to the DB at checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// The DB row ID for this task — used when persisting state
    pub id: i64,

    /// The original user request, verbatim
    pub goal: String,

    /// The Orchestrator's current plan as an ordered list of steps
    pub plan: Vec<String>,

    /// Completed steps with their outcomes (capped to last 10 to stay lean)
    pub progress: Vec<CompletedStep>,

    /// What the Orchestrator is currently working on
    pub current_step: Option<String>,

    /// Persistent key-value state scoped to this task.
    /// e.g. "jobs_applied" = "12", "current_target" = "Acme Corp"
    pub working_memory: HashMap<String, String>,

    /// Set to true when mark_complete tool is called.
    /// The main loop checks this after every tool dispatch.
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedStep {
    pub description: String,
    pub outcome: String,
}

impl Task {
    pub fn new(goal: String, id: i64) -> Self {
        Self {
            id,
            goal,
            plan: Vec::new(),
            progress: Vec::new(),
            current_step: None,
            working_memory: HashMap::new(),
            complete: false,
        }
    }

    /// Record a completed step. Keeps only the last 10 to avoid bloat.
    pub fn record_progress(&mut self, description: String, outcome: String) {
        let desc = if description.is_empty() {
            self.current_step.take().unwrap_or_else(|| "Step".to_string())
        } else {
            description
        };

        self.progress.push(CompletedStep { description: desc, outcome });

        // Keep the list lean — only the last 10 steps matter for context
        if self.progress.len() > 10 {
            self.progress.drain(0..self.progress.len() - 10);
        }
    }

    pub fn remember(&mut self, key: String, value: String) {
        self.working_memory.insert(key, value);
    }

    pub fn recall(&self, key: &str) -> Option<&String> {
        self.working_memory.get(key)
    }

    /// Compact state summary injected into the system prompt after a context prune.
    /// Gives the model full situational awareness without replaying history.
    pub fn state_summary(&self) -> String {
        let mut parts = vec![format!("Goal: {}", self.goal)];

        if !self.plan.is_empty() {
            parts.push("Plan:".to_string());
            for (i, step) in self.plan.iter().enumerate() {
                parts.push(format!("  {}. {}", i + 1, step));
            }
        }

        if !self.progress.is_empty() {
            parts.push(format!("Completed {} steps:", self.progress.len()));
            for step in &self.progress {
                parts.push(format!("  ✓ {} → {}", step.description, step.outcome));
            }
        }

        if let Some(ref current) = self.current_step {
            parts.push(format!("Currently working on: {}", current));
        }

        if !self.working_memory.is_empty() {
            parts.push("Working memory:".to_string());
            for (k, v) in &self.working_memory {
                parts.push(format!("  {}: {}", k, v));
            }
        }

        parts.join("\n")
    }
}