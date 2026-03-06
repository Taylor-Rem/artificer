use crate::pool::GpuHandle;
use crate::api::events::EventSender;
use super::ExecutionType;

pub struct AgentContext {
    pub device_id: u64,
    pub device_key: String,
    pub conversation_id: u64,
    pub parent_task_id: Option<u64>,
    pub gpu: GpuHandle,
    pub events: Option<EventSender>,
    pub execution_type: ExecutionType,
}
