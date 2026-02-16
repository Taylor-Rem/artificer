use std::sync::Arc;
use tokio::sync::RwLock;
use crate::task::Task;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<StateInner>>,
}

struct StateInner {
    device_id: i64,
    interactive_task: Task,
    background_task: Option<Task>,
}

impl AppState {
    pub fn new(device_id: i64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(StateInner {
                device_id,
                interactive_task: Task::Chat,
                background_task: None,
            }))
        }
    }

    pub async fn device_id(&self) -> i64 {
        self.inner.read().await.device_id
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