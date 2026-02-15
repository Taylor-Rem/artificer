pub enum Strength {
    Power,
    Speed,
}

impl Strength {
    pub fn url(&self) -> &'static str {
        match self {
            Strength::Power => "http://localhost:11435/api/chat",
            Strength::Speed => "http://localhost:11434/api/chat",
        }
    }
}

pub enum Capability {
    Reasoner,
    ToolCaller,
    Quick,
    Coder,
}
