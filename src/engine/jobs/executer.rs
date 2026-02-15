use serde_json::Value;
use crate::agent::Agent;
use crate::engine::db::Db;
use crate::engine::jobs::{summarization, title_generation};
use crate::schema::Task;
use crate::services::title::Title;

pub struct JobContext<'a> {
    pub db: &'a Db,
    pub agent: &'a Agent,
    pub title_service: &'a Title,
}

pub async fn execute(task: &Task, ctx: &JobContext<'_>, args: &Value) -> anyhow::Result<String> {
    match task {
        Task::TitleGeneration => title_generation::execute(ctx, args).await,
        Task::Summarization => summarization::execute(ctx, args).await,
        _ => Err(anyhow::anyhow!("Task not implemented: {:?}", task)),
    }
}
