use std::sync::Arc;
use artificer_shared::db::Db;
use crate::pool::GpuHandle;

pub struct AgentContext {
    pub device_id: u64,
    pub conversation_id: u64,
    pub gpu: GpuHandle,
    pub db: Arc<Db>
}