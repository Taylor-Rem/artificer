mod schema;
pub mod implementations;

use anyhow::Result;
use reqwest::Client;
use artificer_shared::{Message, Tool};
pub use schema::{AgentResponse, AgentContext, AgentRoles};

pub struct Agent {
    pub name: &'static str,
    pub system_prompt: String,
    pub tools: Option<Vec<Tool>>,
    pub requires_full_context: bool,
}

impl Agent {
    fn execute(&self, goal: &str, context: AgentContext) -> Result<AgentResponse> {
        let mut messages = self.build_messages(goal, context.conversation.clone());
        let client = Client::new();
        loop {
            let response = self.call_model();
            AgentResponse::complete("ok".to_string());
        }
    }
    fn build_messages(&self, goal: &str, conversation: Vec<Message>) -> Vec<Message> {
        let mut messages = vec![Message {
            role: "system".to_string(),
            content: Some(self.system_prompt()),
            tool_calls: None
        }];
        if self.requires_full_context {
            messages.extend(conversation);
        } else {
            messages.push(Message {
                role: "user".to_string(),
                content: Some(goal.to_string()),
                tool_calls: None
            });
        }
        messages
    }
    fn call_model(&self) -> AgentResponse {

    };
}