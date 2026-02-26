#[macro_export]
macro_rules! define_agents {
    (
        $(
            $name:ident: $role:expr => {
                description: $desc:literal,
                execution_mode: $exec_mode:expr,
                system_prompt: $prompt:expr,
                toolbelts: [$($toolbelt:literal),* $(,)?],
                $(task_tools: $has_task_tools:expr,)?
            }
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum AgentType {
            $($name),*
        }

        impl AgentType {
            pub fn all() -> &'static [AgentType] {
                &[$(AgentType::$name),*]
            }

            pub fn build(self) -> $crate::agent::Agent {
                match self {
                    $(
                        AgentType::$name => {
                            // Collect tools from all specified toolbelts
                            let mut tools = vec![];
                            $(
                                let toolbelt_tools = artificer_shared::get_tools_for(&[$toolbelt]);
                                tools.extend(toolbelt_tools);
                            )*

                            // Conditionally add task management tools
                            $(
                                if $has_task_tools {
                                    use crate::agent::schema::task::TASK_TOOLS;
                                    let task_tools: Vec<artificer_shared::Tool> = TASK_TOOLS
                                        .iter()
                                        .map(|schema| schema.to_tool())
                                        .collect();
                                    tools.extend(task_tools);
                                }
                            )?

                            $crate::agent::Agent {
                                name: stringify!($name),
                                description: $desc,
                                role: $role,
                                execution_mode: $exec_mode,
                                system_prompt: $prompt,
                                tools,
                            }
                        }
                    ),*
                }
            }
        }
    };
}
