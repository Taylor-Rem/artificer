use artificer_shared::Message;
use crate::pool::GpuHandle;

pub struct AgentContext {
    pub gpu: GpuHandle,
    pub conversation: Vec<Message>
}