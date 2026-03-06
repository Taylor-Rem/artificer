mod schema;
pub mod implementations;
pub mod macros;
pub mod execution;
mod llm_types;
mod llm_client;
mod delegation_tools;

use artificer_shared::Tool;
pub use schema::{AgentContext, AgentResponse, AgentRoles, ExecutionMode, ExecutionType, Task};
pub use implementations::AgentType;
pub use execution::AgentExecution;
pub use execution::ToolExecutionContext;

#[derive(Debug, Clone)]
pub struct Agent {
    pub name: &'static str,
    pub description: &'static str,
    pub role: AgentRoles,
    pub execution_mode: ExecutionMode,
    pub system_prompt: &'static str,
    pub tools: Vec<Tool>,
}

impl Agent {
    pub fn build_system_prompt(&self, task_state: &str) -> String {
        let mut prompt = String::new();

        // Stage 1: Base prompt by role
        match self.role {
            AgentRoles::Orchestrator => {
                prompt.push_str(include_str!("prompts/orchestrator_base.txt"));
            }
            AgentRoles::Specialist | AgentRoles::Background => {
                prompt.push_str(include_str!("prompts/specialist_base.txt"));
            }
        }

        prompt.push_str("\n\n");

        // Stage 2: Specialist-specific prompt
        prompt.push_str(self.system_prompt);
        prompt.push_str("\n\n");

        // Stage 3: Available tools
        prompt.push_str("# Available Tools\n\n");
        prompt.push_str(&self.format_tools());
        prompt.push_str("\n\n");

        // Stage 4: Current task state
        prompt.push_str("# Current Task State\n\n");
        prompt.push_str(task_state);

        prompt
    }

    fn format_tools(&self) -> String {
        if self.tools.is_empty() {
            return "No tools available.".to_string();
        }

        let mut output = String::new();
        for tool in &self.tools {
            output.push_str(&format!("## {}\n", tool.function.name));
            output.push_str(&format!("{}\n\n", tool.function.description));

            if let Some(params) = tool.function.parameters.get("properties") {
                if let Some(obj) = params.as_object() {
                    output.push_str("Parameters:\n");
                    for (name, details) in obj {
                        let desc = details.get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let type_name = details.get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        output.push_str(&format!("- {} ({}): {}\n", name, type_name, desc));
                    }
                    output.push('\n');
                }
            }
        }
        output
    }
}
