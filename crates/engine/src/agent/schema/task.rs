use crate::agent::AgentContext;

enum TaskStatus {
    NotStarted,
    InProgress,
    Completed,
    Failed
}
pub struct Task {
    id: u64,
    user_goal: String,
    agent_goal: Option<String>,
    plan: Option<Vec<TaskStep>>,
    current_step: Option<TaskStep>,
    progress: TaskStatus
}

pub struct TaskStep {
    goal: String,
    progress: TaskStatus
}

impl Task {
    pub fn new(context: AgentContext, goal: &str) -> Self {
        let task_id = context.db.create_task(context.device_id, context.conversation_id, &goal).unwrap();
        Self {
            id: task_id,
            user_goal: goal.to_string(),
            agent_goal: None,
            plan: None,
            current_step: None,
            progress: TaskStatus::NotStarted
        }
    }
    fn update_task(&self) -> Task {

    }
}