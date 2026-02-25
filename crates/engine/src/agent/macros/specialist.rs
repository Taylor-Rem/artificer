#[macro_export]
macro_rules! define_agents {
    (
        $(
            $name:ident: $role:expr => {
                description: $desc:literal,
                system_prompt: $prompt:expr,
                tools: $tools:expr,
                $(task_tools: $has_task_tools:expr,)?
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum AgentType {
            $($name),*
        }

        impl AgentType {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name)),*
                }
            }

            pub fn from_str(s: &str) -> Option<Self> {
                match s {
                    $(stringify!($name) => Some(Self::$name),)*
                    _ => None,
                }
            }

            // Generate list of all agent types at compile time
            pub const fn all() -> &'static [AgentType] {
                &[$(AgentType::$name),*]
            }
        }

        impl Agent {
            // Agent is now stateless - no context or task
            pub fn new(agent_type: AgentType) -> Self {
                match agent_type {
                    $(
                        AgentType::$name => {
                            let mut tools = $tools.unwrap_or_default();

                            // Conditionally merge task tools
                            $(
                                if $has_task_tools {
                                    let task_tools: Vec<Tool> = TASK_TOOLS
                                        .iter()
                                        .map(|def| Tool::from(def))
                                        .collect();
                                    tools.extend(task_tools);
                                }
                            )?

                            Agent {
                                name: stringify!($name),
                                description: $desc,
                                role: $role,
                                system_prompt: $prompt,
                                tools,
                            }
                        }
                    ),*
                }
            }

            // Execute with context and goal passed in
            pub fn execute(&self, context: AgentContext, goal: String) -> Result<AgentResponse> {
                let task = Task::new(&context, &goal);
                // ... execution logic
            }
        }
    };
}