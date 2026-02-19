use crate::{register_toolbelt, ToolLocation};

pub struct Router;

impl Default for Router {
    fn default() -> Self {
        Self
    }
}

register_toolbelt! {
    Router {
        description: "Task planning and routing",
        location: ToolLocation::Server,
        tools: {
            "plan_tasks" => plan_tasks {
                description: "Plan a pipeline of tasks to fulfill the user's request",
                params: [
                    "steps": "array" => "Ordered list of steps, each with 'task' (task name) and 'directions' (specific instructions for that task)"
                ]
            }
        }
    }
}

impl Router {
    fn plan_tasks(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        // Just echo the plan back as JSON — the engine will parse and execute it
        Ok(args["steps"].to_string())
    }
}