use crate::{register_toolbelt, ToolLocation};

pub struct Examiner;

impl Default for Examiner {
    fn default() -> Self { Self }
}

register_toolbelt! {
    Examiner {
        description: "Quality examination and reporting",
        location: ToolLocation::Server,
        tools: {
            "report" => report {
                description: "Report whether the user's request has been fulfilled",
                params: [
                    "fulfilled": "boolean" => "Whether the request was fully satisfied",
                    "reason": "string" => "Brief explanation of your determination"
                ]
            }
        }
    }
}

impl Examiner {
    fn report(&self, args: &serde_json::Value) -> anyhow::Result<String> {
        Ok(args.to_string())
    }
}