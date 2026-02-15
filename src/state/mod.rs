use std::sync::Arc;
use tokio::sync::RwLock;
use crate::task::Task;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<StateInner>>,
}

struct StateInner {
    interactive_task: Task,
    background_task: Option<Task>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(StateInner {
                interactive_task: Task::Chat,
                background_task: None,
            }))
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn current_task(&self) -> Task {
        self.inner.read().await.interactive_task.clone()
    }

    pub async fn switch_task(&self, new_task: Task) {
        self.inner.write().await.interactive_task = new_task;
    }

    pub async fn current_background_task(&self) -> Option<Task> {
        self.inner.read().await.background_task.clone()
    }

    pub async fn set_background_task(&self, task: Option<Task>) {
        self.inner.write().await.background_task = task;
    }
}