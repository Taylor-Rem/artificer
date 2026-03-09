use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use artificer_shared::tools::get_tool_schema;
use crate::pool::AgentPool;
use crate::agent::schema::task::{handle_task_tool, is_task_tool};
use super::tool_validation::validate_tool_call;
use crate::agent::{AgentContext, Task};

/// Per-turn context for routing and executing tool calls.
pub struct ToolExecutionContext<'a> {
    pub task: &'a mut Task,
    pub context: &'a AgentContext,
    pub pool: &'a Arc<AgentPool>,
}

impl<'a> ToolExecutionContext<'a> {
    pub fn new(task: &'a mut Task, context: &'a AgentContext, pool: &'a Arc<AgentPool>) -> Self {
        Self { task, context, pool }
    }

    /// Execute any tool call — validates, routes, and emits events.
    pub async fn execute_tool(&mut self, tool_name: &str, args: &Value) -> Result<String> {
        // Validate (skips task::, delegate::, and response:: tools)
        validate_tool_call(tool_name, args)?;

        // Emit tool call event
        if let Some(events) = &self.context.events {
            events.tool_call(
                &format!("task_{}", self.task.id()),
                tool_name,
                args.clone(),
            );
        }

        // Route to appropriate handler
        let result = if is_task_tool(tool_name) {
            handle_task_tool(self.task, tool_name, args)
        } else if tool_name.starts_with("delegate::") {
            self.execute_delegation(tool_name, args).await
        } else {
            self.pool
                .tool_executor()
                .execute(
                    tool_name,
                    args,
                    self.context.device_id as i64,
                    &self.context.device_key,
                )
                .await
        };

        // Emit tool result event
        if let Some(events) = &self.context.events {
            match &result {
                Ok(res) => {
                    events.tool_result(
                        &format!("task_{}", self.task.id()),
                        tool_name,
                        res.clone(),
                    );
                }
                Err(e) => {
                    events.tool_result(
                        &format!("task_{}", self.task.id()),
                        tool_name,
                        format!("ERROR: {}", e),
                    );
                }
            }
        }

        result
    }

    /// Delegate a goal to a specialist agent and return its response.
    async fn execute_delegation(&mut self, tool_name: &str, args: &Value) -> Result<String> {
        let specialist_name_raw = tool_name
            .strip_prefix("delegate::")
            .ok_or_else(|| anyhow::anyhow!("Invalid delegation tool name: {}", tool_name))?;

        let agent_name = Self::normalize_specialist_name(specialist_name_raw);

        // Validate specialist exists
        self.pool
            .get(&agent_name)
            .ok_or_else(|| anyhow::anyhow!("Specialist '{}' not found", agent_name))?;

        let goal = args["goal"]
            .as_str()
            .or_else(|| args["request"].as_str())
            .or_else(|| args["task"].as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing goal/request/task in delegation args"))?;

        // Emit task switch event
        if let Some(events) = &self.context.events {
            events.task_switch(
                &format!("task_{}", self.task.id()),
                &format!("specialist_{}", agent_name),
            );
        }

        let specialist_context = crate::agent::AgentContext {
            device_id: self.context.device_id,
            device_key: self.context.device_key.clone(),
            conversation_id: self.task.conversation_id(),
            parent_task_id: Some(self.task.id()),
            gpu: self.task.gpu().clone(),
            events: self.context.events.clone(),
        };

        // Look up specialist again for AgentExecution::new
        let specialist = self.pool.get(&agent_name).unwrap();
        let execution = crate::agent::AgentExecution::new(
            specialist,
            specialist_context,
            goal,
            self.pool,
        );

        let response = execution.execute(Arc::clone(self.pool)).await?;

        // Emit task switch back event
        if let Some(events) = &self.context.events {
            events.task_switch(
                &format!("specialist_{}", agent_name),
                &format!("task_{}", self.task.id()),
            );
        }

        Ok(response.content)
    }

    /// Convert snake_case specialist name to PascalCase: "file_smith" -> "FileSmith"
    fn normalize_specialist_name(name: &str) -> String {
        name.split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect()
    }

    /// Execute a task management tool directly (no validation overhead).
    pub fn execute_task_tool(&mut self, tool_name: &str, args: &Value) -> Result<String> {
        handle_task_tool(self.task, tool_name, args)
    }

    /// Check whether a tool is available for this agent to call.
    pub fn is_tool_available(&self, tool_name: &str) -> bool {
        if is_task_tool(tool_name)
            || tool_name.starts_with("delegate::")
            || tool_name.starts_with("response::")
        {
            return true;
        }
        get_tool_schema(tool_name).is_ok()
    }
}
