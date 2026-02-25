use std::collections::HashMap;
use std::sync::Arc;
use crate::agent::{Agent, AgentType};

pub struct AgentPool {
    agents: HashMap<&'static str, Agent>,
}

impl AgentPool {
    pub fn new() -> Self {
        let mut agents = HashMap::new();

        // Automatically populate from all agent types
        for agent_type in AgentType::all() {
            let agent = Agent::new(*agent_type);
            agents.insert(agent.name, agent);
        }

        Self { agents }
    }

    pub fn get(&self, name: &str) -> Option<&Agent> {
        self.agents.get(name)
    }

    pub fn all(&self) -> impl Iterator<Item = &Agent> {
        self.agents.values()
    }
}

impl Default for AgentPool {
    fn default() -> Self {
        Self::new()
    }
}