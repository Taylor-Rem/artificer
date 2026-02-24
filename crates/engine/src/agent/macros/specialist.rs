#[macro_export]
macro_rules! define_specialist {
    (
        $struct_name:ident {
            name: $name:literal,
            gpu_role: $gpu_role:expr,
            toolbelts: [$($toolbelt:literal),* $(,)?],
            instructions: $instructions:literal,

            // Optional: custom mode detection logic
            $(mode_detection: $mode_fn:expr,)?
        }
    ) => {
        pub struct $struct_name {
            db_id: i64,
        }

        impl $struct_name {
            pub const DEFINITION: $crate::agent::SpecialistDefinition = $crate::agent::SpecialistDefinition {
                name: $name,
                gpu_role: $gpu_role,
                // Memory toolbelt is automatically prepended to all specialists
                toolbelts: &["Memory", $($toolbelt),*],
                instructions: $instructions,
            };

            pub fn new(db_id: i64) -> Self {
                Self { db_id }
            }

            // Mode detection - use custom or default
            $(
                fn is_proxy_mode(&self, goal: &str, tool_call: &$crate::ToolCall) -> anyhow::Result<bool> {
                    $mode_fn(goal, tool_call)
                }
            )?

            #[allow(dead_code)]
            fn is_proxy_mode_default(&self, goal: &str, _tool_call: &$crate::ToolCall) -> anyhow::Result<bool> {
                let goal_lower = goal.to_lowercase();

                // Agentic keywords override - if present, definitely not proxy mode
                let agentic = ["analyze", "review", "find", "search", "compare",
                              "refactor", "reorganize", "explain", "summarize"];
                if agentic.iter().any(|kw| goal_lower.contains(kw)) {
                    return Ok(false);
                }

                // Default to agentic when uncertain
                Ok(false)
            }

            async fn handle_memory_tool(
                &self,
                tool_name: &str,
                args: &serde_json::Value,
                task: &$crate::agent::schema::Task,
                context: &$crate::agent::schema::AgentContext,
            ) -> anyhow::Result<String> {
                match tool_name {
                    "Memory::commit" => {
                        let key = args["key"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing key"))?;
                        let value = args["value"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing value"))?;
                        let memory_type = args["memory_type"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing memory_type"))?;
                        let confidence = args["confidence"].as_f64().unwrap_or(1.0);

                        context.db.upsert_memory(
                            context.device_id,
                            Some(self.db_id),
                            Some(task.id),
                            key,
                            value,
                            memory_type,
                            confidence,
                        )?;

                        Ok(format!("Committed to memory: {} = {}", key, value))
                    }

                    "Memory::recall" => {
                        let key = args["key"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing key"))?;

                        let result = context.db.query_row_optional(
                            "SELECT value FROM local_data
                             WHERE device_id = ?1 AND specialist_id = ?2 AND key = ?3",
                            artificer_shared::rusqlite::params![context.device_id, self.db_id, key],
                            |row| row.get::<_, String>(0),
                        )?;

                        Ok(result.unwrap_or_else(|| format!("No memory found for key '{}'", key)))
                    }

                    "Memory::search" => {
                        let pattern = args["pattern"].as_str()
                            .ok_or_else(|| anyhow::anyhow!("Missing pattern"))?;

                        context.db.query(
                            "SELECT key, value, memory_type, confidence
                             FROM local_data
                             WHERE device_id = ?1 AND specialist_id = ?2
                             AND (key LIKE '%' || ?3 || '%' OR value LIKE '%' || ?3 || '%')
                             ORDER BY confidence DESC, updated_at DESC",
                            artificer_shared::rusqlite::params![context.device_id, self.db_id, pattern],
                        )
                    }

                    _ => Err(anyhow::anyhow!("Unknown memory tool: {}", tool_name))
                }
            }
        }

        impl $crate::agent::Agent for $struct_name {
            fn name(&self) -> &str {
                Self::DEFINITION.name
            }

            fn system_prompt(&self, memory_context: Option<&str>) -> String {
                $crate::agent::schema::system_prompt::build_specialist_prompt(
                    Self::DEFINITION.name,
                    Self::DEFINITION.instructions,
                    Self::DEFINITION.toolbelts,
                    memory_context,
                )
            }

            fn available_tools(&self) -> Vec<$crate::Tool> {
                // Memory toolbelt already included in DEFINITION.toolbelts
                artificer_shared::tools::get_tools_for(Self::DEFINITION.toolbelts)
            }

            async fn dispatch(
                &self,
                tool_call: &$crate::ToolCall,
                task: &mut $crate::agent::schema::Task,
                context: &$crate::agent::schema::AgentContext,
            ) -> anyhow::Result<String> {
                let tool_name = &tool_call.function.name;
                let args = &tool_call.function.arguments;

                if let Some(ref ev) = context.events {
                    ev.tool_call(self.name(), tool_name, args.clone());
                }

                // Handle memory tools specially (need specialist_id context)
                let result = if tool_name.starts_with("Memory::") {
                    self.handle_memory_tool(tool_name, args, task, context).await?
                } else {
                    // Use the context's executor for all other tools
                    context.executor.execute(tool_name, args).await?
                };

                if let Some(ref ev) = context.events {
                    ev.tool_result(self.name(), tool_name, result.clone());
                }

                // Check if we should mark complete (proxy mode)
                let is_proxy = self.is_proxy_mode(&task.goal, tool_call)
                    .unwrap_or(false);
                if is_proxy {
                    task.complete = true;
                }

                Ok(result)
            }

            fn create_task(&self, goal: &str, context: &$crate::agent::schema::AgentContext) -> anyhow::Result<$crate::agent::schema::Task> {
                let task_id = context.db.create_task(
                    context.device_id,
                    context.conversation_id,
                    goal,
                    self.db_id,
                    context.parent_task_id,
                )?;
                Ok($crate::agent::schema::Task::new(goal.to_string(), task_id))
            }

            async fn execute(
                &self,
                goal: String,
                context: $crate::agent::schema::AgentContext,
                gpu: &$crate::pool::GpuHandle,
                events: Option<&$crate::api::events::EventSender>,
                client: &reqwest::Client,
            ) -> anyhow::Result<$crate::agent::schema::AgentResponse> {
                // TODO: Implement shared execution loop
                // This will be the plan-execute-observe-iterate loop
                // For now, placeholder
                todo!("Shared execution loop not yet implemented")
            }
        }
    };
}