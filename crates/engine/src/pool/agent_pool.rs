use std::collections::HashMap;
use std::sync::Arc;
use reqwest::Client;
use artificer_shared::db::Db;
use artificer_shared::executor::ToolExecutor;
use crate::agent::{Agent, AgentType};

pub struct AgentPool {
    agents: HashMap<&'static str, Agent>,
    pub client: Client,
    pub db: Arc<Db>,
    pub tool_executor: Arc<ToolExecutor>,
}

impl AgentPool {
    pub fn new(db: Arc<Db>, tool_executor: Arc<ToolExecutor>) -> Self {
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

        Self {
            agents,
            client,
            db,
            tool_executor,
        }
    }

    pub fn get(&self, name: &str) -> Option<&Agent> {
        self.agents.get(name)
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn db(&self) -> &Arc<Db> {
        &self.db
    }

    pub fn tool_executor(&self) -> &Arc<ToolExecutor> {
        &self.tool_executor
    }
}
