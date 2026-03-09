// crates/engine/src/agent/implementations/orchestrator.rs

use std::sync::Arc;
use anyhow::Result;
use reqwest::Client;

use artificer_shared::db::Db;
use crate::api::events::EventSender;
use crate::pool::GpuHandle;
use crate::{Agent, SpecialistDefinition};
use crate::agent::schema::{Task, AgentContext, AgentResponse};

/// The primary orchestrator - coordinates all work by delegating to specialists
pub struct Orchestrator {
    db: Arc<Db>,
    device_id: i64,
}

impl Orchestrator {
    pub fn new(db: Arc<Db>, device_id: i64) -> Self {
        Self { db, device_id }
    }

    /// Load long-term memory for this device (all specialists' memories)
    fn load_memory_context(&self) -> Result<Option<String>> {
        let raw = self.db.get_memory(self.device_id)?;
        let memories: Vec<serde_json::Value> = serde_json::from_str(&raw)?;
        if memories.is_empty() {
            return Ok(None);
        }

        let parts: Vec<String> = memories
            .iter()
            .filter_map(|m| {
                let key = m["key"].as_str()?;
                let value = m["value"].as_str()?;
                let mem_type = m["memory_type"].as_str().unwrap_or("fact");
                Some(format!("[{}] {}: {}", mem_type, key, value))
            })
            .collect();

        Ok(if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        })
    }
}

impl Agent for Orchestrator {
    fn name(&self) -> &str {
        "orchestrator"
    }

    fn system_prompt(&self, memory_context: Option<&str>) -> String {
        crate::agent::schema::system_prompt::build_orchestrator_prompt(
            None, // No task state yet - we're just starting
            memory_context,
        )
    }

    fn available_tools(&self) -> Vec<artificer_shared::Tool> {
        // TODO: Return working_memory tools + delegation tools
        vec![]
    }

    async fn execute(
        &self,
        goal: String,
        context: AgentContext,
        gpu: &GpuHandle,
        events: Option<&EventSender>,
        client: &Client,
    ) -> Result<AgentResponse> {
        // TODO: Implement the full orchestrator execution loop
        // This is where the plan-execute-observe-iterate happens
        todo!("Orchestrator execution loop not yet implemented")
    }

    async fn dispatch(
        &self,
        tool_call: &artificer_shared::ToolCall,
        task: &mut Task,
        context: &AgentContext,
    ) -> Result<String> {
        let tool_name = &tool_call.function.name;
        let args = &tool_call.function.arguments;

        // TODO: Handle working_memory tools and delegation
        match tool_name.as_str() {
            name if name.starts_with("working_memory::") => {
                // Handle working memory operations
                todo!("Working memory tools not yet implemented")
            }
            name if name.starts_with("delegate::") => {
                // Delegate to a specialist
                todo!("Specialist delegation not yet implemented")
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name))
        }
    }

    fn create_task(&self, goal: &str, context: &AgentContext) -> Result<Task> {
        // Orchestrator creates primary tasks (no parent_task_id)
        let task_id = context.db.create_task(
            context.device_id,
            context.conversation_id,
            goal,
            1, // specialist_id = 1 for orchestrator (needs to be in specialists table)
            None, // No parent task - this is a primary task
        )?;
        Ok(Task::new(goal.to_string(), task_id))
    }
}