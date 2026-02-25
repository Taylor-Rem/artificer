mod schema;
pub mod implementations;
pub mod macros;

use anyhow::Result;
use reqwest::Client;
use artificer_shared::{Message, Tool};
pub use schema::{AgentResponse, AgentContext, AgentRoles, Task};
pub use implementations::AgentType;

pub struct Agent {
    pub name: &'static str,
    description: &'static str,
    role: AgentRoles,
    pub system_prompt: &'static str,
    pub tools: Option<Vec<Tool>>,
}

impl Agent {
    fn execute(&self, goal: &str, context: AgentContext) -> Result<AgentResponse> {
        let task = Task::new(context, goal);
        let mut messages = self.build_messages(goal);
        let client = Client::new();
        loop {
            // let response = self.call_model();
            AgentResponse::complete("ok".to_string());
        }
    }
    fn build_messages(&self, goal: &str) -> Vec<Message> {
        match self.role {
            AgentRoles::Orchestrator => {
                let mut messages = self.context.db
                    .get_messages(self.context.conversation_id)
                    .unwrap_or_default();
                if messages.is_empty() {
                    vec!(
                        Message {
                            role: "system".to_string(),
                            content: Some(self.build_system_prompt()),
                            tool_calls: None
                        },
                        Message {
                            role: "user".to_string(),
                            content: Some(goal.to_string()),
                            tool_calls: None
                        }
                    )
                } else {
                    messages.push(
                        Message {
                            role: "user".to_string(),
                            content: Some(goal.to_string()),
                            tool_calls: None
                        }
                    );
                    messages
                }

            },
            AgentRoles::Specialist => {
                vec!(
                    Message {
                        role: "system".to_string(),
                        content: Some(self.build_system_prompt()),
                        tool_calls: None
                    },
                    Message {
                        role: "user".to_string(),
                        content: Some(goal.to_string()),
                        tool_calls: None
                    }
                )
            }
        }
    }
    // fn call_model(&self) -> AgentResponse {
    //
    // };

    fn build_system_prompt(&self) -> String {
        let mut system_prompt = match self.role {
            AgentRoles::Orchestrator => schema::prompts::ORCHESTRATOR_BASE,
            AgentRoles::Specialist => schema::prompts::SPECIALIST_BASE,
        }.to_string();

        system_prompt.push('\n');
        system_prompt.push_str(&self.system_prompt);
        system_prompt
    }
}