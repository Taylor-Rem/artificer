use std::sync::Arc;
use artificer_shared::db::Db;
use artificer_shared::executor::ToolExecutor;
use crate::api::events::EventSender;

/// Execution context passed to agents containing database, device info,
/// and tool execution strategy.
pub struct AgentContext {
    pub db: Arc<Db>,
    pub device_id: i64,
    pub conversation_id: u64,
    pub parent_task_id: Option<i64>,
    pub events: Option<EventSender>,

    /// Tool execution strategy (local for server-side, remote for client-side)
    pub executor: ToolExecutor,
}

impl AgentContext {
    /// Create context for the orchestrator (server-side tool execution)
    pub fn for_orchestrator(
        db: Arc<Db>,
        device_id: i64,
        conversation_id: u64,
    ) -> Self {
        Self {
            db,
            device_id,
            conversation_id,
            parent_task_id: None,
            events: None,
            executor: ToolExecutor::local(),
        }
    }

    /// Create context for a specialist (may need remote tool execution)
    pub fn for_specialist(
        db: Arc<Db>,
        device_id: i64,
        conversation_id: u64,
        parent_task_id: i64,
        envoy_url: String,
        device_key: String,
    ) -> Self {
        Self {
            db,
            device_id,
            conversation_id,
            parent_task_id: Some(parent_task_id),
            events: None,
            // Specialists might need to call client-side tools remotely
            executor: ToolExecutor::remote(envoy_url, device_id, device_key),
        }
    }

    /// Add a parent task ID (for creating sub-tasks)
    pub fn with_parent_task(mut self, parent_task_id: i64) -> Self {
        self.parent_task_id = Some(parent_task_id);
        self
    }

    /// Add event streaming
    pub fn with_events(mut self, events: EventSender) -> Self {
        self.events = Some(events);
        self
    }
}