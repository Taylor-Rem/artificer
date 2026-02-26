use anyhow::Result;
use reqwest::Client;
use artificer_shared::Message;
use crate::agent::{schema, Agent, AgentContext, AgentResponse, AgentRoles, Task};

pub struct AgentExecution {
    agent: &'static Agent,
    context: AgentContext,
    task: Task
}

impl AgentExecution {
    pub fn new(agent: &'static Agent, context: AgentContext, goal: &str) -> Self {
        let task = Task::new(&context, &goal);
        Self {agent, context, task}
    }
    pub async fn execute(&self) -> Result<AgentResponse> {
        let mut messages = self.build_initial_messages(&self.task.user_goal);
        loop {
            self.inject_task_state(&mut messages);
            // let response = self.call_model();
            AgentResponse::complete("ok".to_string());
        }
    }
    fn build_initial_messages(&self, goal: &str) -> Vec<Message> {
        match self.agent.role {
            AgentRoles::Orchestrator => {
                let history = self.context.db
                    .get_messages(self.context.conversation_id)
                    .unwrap_or_default();

                if history.is_empty() {
                    // First message in conversation
                    vec![
                        Message {
                            role: "system".to_string(),
                            content: Some(self.build_system_prompt_with_task()),
                            tool_calls: None,
                        },
                        Message {
                            role: "user".to_string(),
                            content: Some(goal.to_string()),
                            tool_calls: None,
                        }
                    ]
                } else {
                    // Continue existing conversation
                    let mut messages = history;
                    // Update system message with task state
                    if let Some(first) = messages.first_mut() {
                        first.content = Some(self.build_system_prompt_with_task());
                    }
                    messages.push(Message {
                        role: "user".to_string(),
                        content: Some(goal.to_string()),
                        tool_calls: None,
                    });
                    messages
                }
            },
            AgentRoles::Specialist | AgentRoles::Background => {
                vec![
                    Message {
                        role: "system".to_string(),
                        content: Some(self.build_system_prompt_with_task()),
                        tool_calls: None,
                    },
                    Message {
                        role: "user".to_string(),
                        content: Some(goal.to_string()),
                        tool_calls: None,
                    }
                ]
            }
        }
    }
    fn inject_task_state(&self, messages: &mut Vec<Message>) {
        if let Some(system_msg) = messages.first_mut() {
            if system_msg.role == "system" {
                // Rebuild system prompt with current task state
                system_msg.content = Some(self.build_system_prompt_with_task());
            }
        }
    }

    fn build_system_prompt_with_task(&self) -> String {
        let mut prompt = self.build_base_system_prompt();

        // Add task state section
        prompt.push_str("\n\n## CURRENT TASK STATE\n");
        prompt.push_str(&self.task.state_summary());

        prompt
    }
    fn build_base_system_prompt(&self) -> String {
        let mut system_prompt = match self.agent.role {
            AgentRoles::Orchestrator => schema::prompts::ORCHESTRATOR_BASE,
            AgentRoles::Specialist => schema::prompts::SPECIALIST_BASE,
            AgentRoles::Background => schema::prompts::BACKGROUND_BASE,
        }.to_string();

        system_prompt.push('\n');
        system_prompt.push_str(self.agent.system_prompt);
        system_prompt
    }
    fn handle_tool_call(&mut self, tool_call: &ToolCall) -> Result<String> {
        use crate::agent::schema::task::{handle_task_tool, is_task_tool};

        let tool_name = &tool_call.function.name;
        let args = &tool_call.function.arguments;

        // Check if it's a task management tool
        if is_task_tool(tool_name) {
            return handle_task_tool(&mut self.task, tool_name, args);
        }

        // Handle other tools (delegates, etc.)
        match tool_name.as_str() {
            "delegate::web_research" => {
                // Delegate to web researcher
                todo!()
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name))
        }
    }
}