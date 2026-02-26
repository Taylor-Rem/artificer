use anyhow::Result;
use serde_json::Value;
use artificer_shared::tools::get_tool_schema;
use crate::pool::AgentPool;
use super::schema::task::{handle_task_tool, is_task_tool};
use super::tool_validation::validate_tool_call;
use super::{AgentContext, Task};

/// Per-turn context for routing and executing tool calls.
///
/// Holds references to the mutable task state, request context,
/// and shared pool so it can route any tool call to the right handler.
pub struct ToolExecutionContext<'a> {
    task: &'a mut Task,
    context: &'a AgentContext,
    pool: &'a AgentPool,
}

impl<'a> ToolExecutionContext<'a> {
    pub fn new(task: &'a mut Task, context: &'a AgentContext, pool: &'a AgentPool) -> Self {
        Self { task, context, pool }
    }

    /// Execute any tool call — validates first, then routes to task tools
    /// or external (server/client) tools.
    pub async fn execute_tool(&mut self, tool_name: &str, args: &Value) -> Result<String> {
        validate_tool_call(tool_name, args)?;

        if is_task_tool(tool_name) {
            handle_task_tool(self.task, tool_name, args)
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
        }
    }

    /// Execute a task management tool directly (no validation overhead).
    pub fn execute_task_tool(&mut self, tool_name: &str, args: &Value) -> Result<String> {
        handle_task_tool(self.task, tool_name, args)
    }

    /// Check whether a tool is available for this agent to call.
    pub fn is_tool_available(&self, tool_name: &str) -> bool {
        if is_task_tool(tool_name) {
            return true;
        }
        get_tool_schema(tool_name).is_ok()
    }

    /// Format a tool result for inclusion in the LLM message history.
    pub fn format_tool_result(tool_name: &str, result: &str, is_error: bool) -> String {
        if is_error {
            format!("[Tool Error - {}]: {}", tool_name, result)
        } else {
            result.to_string()
        }
    }

    /// Returns true if a formatted tool result string indicates an error.
    pub fn is_error_result(result: &str) -> bool {
        result.starts_with("[Tool Error")
    }
}
