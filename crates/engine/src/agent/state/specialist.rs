use serde_json::Value;
use super::{TaskState, AgentState};

/// A single tool call and its result, tracked by index.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub index: usize,
    pub tool_name: String,
    pub tool_args: Value,
    pub tool_result: String,
}

/// Execution state for a specialist's fixed message window.
/// Ephemeral — not persisted. Lives for one delegation only.
pub struct SpecialistExecution {
    pub task: TaskState,
    pub tool_calls: Vec<ToolCallRecord>,
    pub response_vec: Vec<usize>,
    pub return_signaled: bool,
    pub response_message: Option<String>,
}

impl SpecialistExecution {
    pub fn new(task: TaskState) -> Self {
        Self {
            task,
            tool_calls: Vec::new(),
            response_vec: Vec::new(),
            return_signaled: false,
            response_message: None,
        }
    }

    /// Record a new tool call and its result. Returns the assigned index (1-based).
    pub fn record_tool_call(&mut self, tool_name: String, tool_args: Value, tool_result: String) -> usize {
        let index = self.tool_calls.len() + 1;
        self.tool_calls.push(ToolCallRecord { index, tool_name, tool_args, tool_result });
        index
    }

    /// Add a tool call result to the response buffer by index. Does NOT signal return.
    pub fn add_to_response_vec(&mut self, index: usize) -> Result<String, String> {
        if index == 0 || index > self.tool_calls.len() {
            return Err(format!("Invalid tool call index: {}. Valid range: 1-{}", index, self.tool_calls.len()));
        }
        if self.response_vec.contains(&index) {
            return Err(format!("Tool call {} is already in response_vec", index));
        }
        self.response_vec.push(index);
        Ok(format!("Added tool call {} to response_vec", index))
    }

    /// Add a tool call result to the response buffer AND signal return.
    pub fn return_with_tool_call(&mut self, index: usize) -> Result<String, String> {
        self.add_to_response_vec(index)?;
        self.return_signaled = true;
        Ok(format!("Added tool call {} to response_vec. Returning to orchestrator.", index))
    }

    /// Signal return with current response_vec contents (no additional tool calls added).
    pub fn return_as_is(&mut self) -> String {
        self.return_signaled = true;
        "Returning to orchestrator with current response_vec.".to_string()
    }

    pub fn should_return(&self) -> bool {
        self.return_signaled
    }

    pub fn set_response_message(&mut self, msg: String) {
        self.response_message = Some(msg);
    }

    /// Force return — used by the safety cap when max iterations are exceeded.
    pub fn force_return(&mut self) {
        self.return_signaled = true;
        if self.response_message.is_none() {
            self.response_message = Some(
                "Max iteration limit reached. Returning with available results.".to_string()
            );
        }
    }

    /// Get the full, untruncated result for a tool call by index.
    pub fn get_full_result(&self, index: usize) -> Result<String, String> {
        if index == 0 || index > self.tool_calls.len() {
            return Err(format!("Invalid index: {}. Valid: 1-{}", index, self.tool_calls.len()));
        }
        Ok(self.tool_calls[index - 1].tool_result.clone())
    }

    /// Build the delegation summary XML returned to the orchestrator.
    /// Uses full (non-truncated) tool results.
    pub fn build_delegation_summary(&self, response_message: &str) -> String {
        let mut xml = String::new();
        xml.push_str("<delegation_summary>\n");
        xml.push_str(&format!("  <response_message>{}</response_message>\n", response_message));
        xml.push_str("  <tool_call_results>\n");
        for &idx in &self.response_vec {
            if let Some(tc) = self.tool_calls.iter().find(|t| t.index == idx) {
                xml.push_str("    <tool_call>\n");
                xml.push_str(&format!("      <index>{}</index>\n", tc.index));
                xml.push_str(&format!("      <tool_name>{}</tool_name>\n", tc.tool_name));
                xml.push_str(&format!("      <tool_args>{}</tool_args>\n", tc.tool_args));
                xml.push_str(&format!("      <tool_result>{}</tool_result>\n", tc.tool_result));
                xml.push_str("    </tool_call>\n");
            }
        }
        xml.push_str("  </tool_call_results>\n");
        xml.push_str("</delegation_summary>");
        xml
    }
}

impl AgentState for SpecialistExecution {
    fn build_state_xml(&self) -> String {
        let mut xml = String::new();

        // 1. Task XML
        xml.push_str(&self.task.build_task_xml());
        xml.push_str("\n\n");

        // 2. next_action block — computed dynamically
        if !self.response_vec.is_empty() {
            xml.push_str(&format!(
                "<next_action>response_vec has {} result(s). Call response::return_as_is to return, or add more.</next_action>\n\n",
                self.response_vec.len()
            ));
        } else if !self.tool_calls.is_empty() {
            xml.push_str(&format!(
                "<next_action>You have {} tool result(s). Select results with response:: tools and return.</next_action>\n\n",
                self.tool_calls.len()
            ));
        }

        // 3. Duplicate tool call warnings (one per unique over-called pair)
        let mut seen_pairs: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        for tc in &self.tool_calls {
            let args_str = tc.tool_args.to_string();
            let pair = (tc.tool_name.clone(), args_str.clone());
            if seen_pairs.contains(&pair) {
                continue;
            }
            let count = self.tool_calls.iter()
                .filter(|t| t.tool_name == tc.tool_name && t.tool_args.to_string() == args_str)
                .count();
            if count > 1 {
                xml.push_str(&format!(
                    "<warning>Tool {} has been called {} times with identical arguments. Do NOT call it again. Use response:: tools to return the result.</warning>\n",
                    tc.tool_name, count
                ));
                seen_pairs.insert(pair);
            }
        }
        if !seen_pairs.is_empty() {
            xml.push('\n');
        }

        // 4. response_vec
        if self.response_vec.is_empty() {
            xml.push_str("<response_vec status=\"EMPTY\"/>\n\n");
        } else {
            xml.push_str(&format!("<response_vec status=\"{} items\">\n", self.response_vec.len()));
            for &idx in &self.response_vec {
                if let Some(tc) = self.tool_calls.iter().find(|t| t.index == idx) {
                    xml.push_str("  <tool_call>\n");
                    xml.push_str(&format!("    <index>{}</index>\n", tc.index));
                    xml.push_str(&format!("    <tool_name>{}</tool_name>\n", tc.tool_name));
                    xml.push_str(&format!("    <tool_args>{}</tool_args>\n", tc.tool_args));
                    let result_display = if tc.tool_result.len() > 4000 {
                        format!("{}... [truncated, {} chars total]", &tc.tool_result[..4000], tc.tool_result.len())
                    } else {
                        tc.tool_result.clone()
                    };
                    xml.push_str(&format!("    <tool_result>{}</tool_result>\n", result_display));
                    xml.push_str("  </tool_call>\n");
                }
            }
            xml.push_str("</response_vec>\n\n");
        }

        // 5. tool_calls
        xml.push_str("<tool_calls>\n");
        for tc in &self.tool_calls {
            xml.push_str("  <tool_call>\n");
            xml.push_str(&format!("    <index>{}</index>\n", tc.index));
            xml.push_str(&format!("    <tool_name>{}</tool_name>\n", tc.tool_name));
            xml.push_str(&format!("    <tool_args>{}</tool_args>\n", tc.tool_args));
            let result_display = if tc.tool_result.len() > 4000 {
                format!("{}... [truncated, {} chars total]", &tc.tool_result[..4000], tc.tool_result.len())
            } else {
                tc.tool_result.clone()
            };
            xml.push_str(&format!("    <tool_result>{}</tool_result>\n", result_display));
            xml.push_str("  </tool_call>\n");
        }
        xml.push_str("</tool_calls>");

        xml
    }

    fn should_terminate(&self) -> bool {
        self.return_signaled || self.task.is_complete()
    }

    fn build_response(&self) -> String {
        let message = self.response_message
            .as_deref()
            .unwrap_or("Task completed.");
        self.build_delegation_summary(message)
    }
}
