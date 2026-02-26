use std::collections::HashMap;
use reqwest::Client;
use crate::agent::{Agent, AgentType};

pub struct AgentPool {
    agents: HashMap<&'static str, Agent>,
    pub client: Client
}

impl AgentPool {
    pub fn new() -> Self {
        let mut agents = HashMap::new();

        for agent_type in AgentType::all() {
            let agent = agent_type.build();
            agents.insert(agent.name, agent);
        }

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .pool_max_idle_per_host(10)
            .build()
            .expect("Failed to build HTTP client");

        Self { agents, client }
    }

    pub fn get(&self, name: &str) -> Option<&Agent> {
        self.agents.get(name)
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl Default for AgentPool {
    fn default() -> Self {
        Self::new()
    }
}