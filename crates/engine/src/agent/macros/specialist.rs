#[macro_export]
macro_rules! define_agents {
    (
        $(
            $name:ident: $role:expr => {
                description: $desc:literal,
                system_prompt: $prompt:expr,
                tools: $tools:expr,
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Copy)]
        pub enum AgentType {
            $($name),*
        }

        impl AgentType {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name).to_lowercase()),*
                }
            }

            pub fn from_str(s: &str) -> Option<Self> {
                match s {
                    $(stringify!($name) => Some(Self::$name),)*
                    _ => None,
                }
            }
        }

        impl Agent {
            pub fn new(agent_type: AgentType, context: AgentContext) -> Self {
                match agent_type {
                    $(
                        AgentType::$name => Agent {
                            name: stringify!($name),
                            description: $desc,
                            role: $role,
                            system_prompt: $prompt.to_string(),
                            tools: $tools,
                            context,
                        }
                    ),*
                }
            }
        }
    };
}