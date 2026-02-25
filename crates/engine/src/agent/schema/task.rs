enum TaskStatus {
    Completed,
    InProgress,
    Failed
}
pub struct Task {
    id: u64,
    user_goal: String,
    agent_goal: String,
    plan: Vec<TaskStep>,
    current_step: TaskStep,
    progress: TaskStatus
}

pub struct TaskStep {
    goal: String,
    progress: TaskStatus
}

impl Task {
    fn create_task(&self) -> Task {

    }
    fn update_task(&self) -> Task {

    }
}