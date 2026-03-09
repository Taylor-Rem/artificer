use serde_json::Value;

/// A single tool call and its result, tracked by index.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub index: usize,
    pub tool_name: String,
    pub tool_args: Value,
    pub tool_result: String,
}

/// Execution state for a specialist's fixed 3-message window.
/// Ephemeral — not persisted. Lives for one delegation only.
pub struct SpecialistState {
    /// Complete history of tool calls made during this delegation.
    tool_calls: Vec<ToolCallRecord>,
    /// Indices into `tool_calls` that should be included in the response.
    response_vec: Vec<usize>,
    /// Whether the specialist has signaled it's done.
    should_return: bool,
    /// The specialist's summary message (set on return).
    pub response_message: Option<String>,
}

impl SpecialistState {
    pub fn new() -> Self {
        Self {
            tool_calls: Vec::new(),
            response_vec: Vec::new(),
            should_return: false,
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
        self.should_return = true;
        Ok(format!("Added tool call {} to response_vec. Returning to orchestrator.", index))
    }

    /// Signal return with current response_vec contents (no additional tool calls added).
    pub fn return_as_is(&mut self) -> String {
        self.should_return = true;
        "Returning to orchestrator with current response_vec.".to_string()
    }

    pub fn should_return(&self) -> bool {
        self.should_return
    }

    pub fn set_response_message(&mut self, msg: String) {
        self.response_message = Some(msg);
    }

    /// Build the XML for message 3 (execution state). Rebuilt from scratch each iteration.
    pub fn build_state_xml(&self, task_xml: &str) -> String {
        let mut xml = String::new();

        if !task_xml.is_empty() {
            xml.push_str(task_xml);
            xml.push_str("\n\n");
        }

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
        xml.push_str("</tool_calls>\n\n");

        xml.push_str("<response_vec>\n");
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
        xml.push_str("</response_vec>\n");

        xml
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
