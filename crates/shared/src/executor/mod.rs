use anyhow::Result;
use serde_json::Value;
use crate::tools::{get_tool_schema, use_tool};
use crate::ToolLocation;

pub enum ToolExecutor {
    Local,
    Remote {
        base_url: String,
        device_id: i64,
        device_key: String,
    },
}

impl ToolExecutor {
    pub fn local() -> Self {
        Self::Local
    }

    pub fn remote(base_url: String, device_id: i64, device_key: String) -> Self {
        Self::Remote { base_url, device_id, device_key }
    }

    pub async fn execute(&self, tool_name: &str, args: &Value) -> Result<String> {
        // Look up the tool's location
        let schema = get_tool_schema(tool_name)?;

        match schema.location {
            ToolLocation::Server => {
                // Always execute server-side tools locally
                use_tool(tool_name, args)
            }
            ToolLocation::Client => {
                // Client-side tools need remote execution if we have a remote endpoint
                match self {
                    Self::Local => {
                        // Can't execute client-side tools locally
                        Err(anyhow::anyhow!(
                            "Tool '{}' requires client-side execution but no remote endpoint configured",
                            tool_name
                        ))
                    }
                    Self::Remote { base_url, device_id, device_key } => {
                        self.execute_remote(base_url, *device_id, device_key, tool_name, args).await
                    }
                }
            }
        }
    }

    async fn execute_remote(
        &self,
        base_url: &str,
        device_id: i64,
        device_key: &str,
        tool_name: &str,
        args: &Value
    ) -> Result<String> {
        let client = reqwest::Client::new();
        let url = format!("{}/shared/execute", base_url);

        let request_body = serde_json::json!({
            "device_id": device_id,
            "device_key": device_key,
            "tool_name": tool_name,
            "arguments": args,
        });

        let response = client
            .post(&url)
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Remote tool execution failed ({}): {}",
                status,
                error_text
            ));
        }

        let result: serde_json::Value = response.json().await?;

        result["result"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("Invalid response from remote executor"))
    }
}