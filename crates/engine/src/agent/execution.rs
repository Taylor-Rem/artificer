use anyhow::Result;
use crate::agent::{Agent, AgentContext, AgentResponse, Task};
use crate::pool::AgentPool;

/// Drives the execution of a single agent for a single goal.
/// Full implementation in Request 5.
#[allow(dead_code)]
pub struct AgentExecution {
    agent_name: String,
    goal: String,
    task: Task,
}

impl AgentExecution {
    pub fn new(agent: &Agent, context: AgentContext, goal: &str, pool: &AgentPool) -> Self {
        let task = Task::new(
            &context,
            context.parent_task_id,
            goal,
            pool.db().clone(),
        );
        Self {
            agent_name: agent.name.to_string(),
            goal: goal.to_string(),
            task,
        }
    }

    /// Execute the agent against the goal.
    /// TODO (Request 5): implement full agentic loop.
    pub async fn execute(self, _pool: &AgentPool) -> Result<AgentResponse> {
        Ok(AgentResponse::complete(format!(
            "Agent '{}' received goal: {}",
            self.agent_name, self.goal
        )))
    }
}
