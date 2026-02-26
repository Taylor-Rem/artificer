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
            pub fn build(self, client: Client) -> Agent {
                match self {
                    $(
                        AgentType::$name => {
                            let mut tools = $tools.unwrap_or_default();

                            // Conditionally merge task tools
                            $(
                                if $has_task_tools {
                                    // Import from task module
                                    use crate::agent::schema::task::TASK_TOOLS;

                                    let task_tools: Vec<Tool> = TASK_TOOLS
                                        .iter()
                                        .map(|schema| schema.to_tool())
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
                                client,
                            }
                        }
                    ),*
                }
            }
        }
    };
}