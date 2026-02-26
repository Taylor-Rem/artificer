use anyhow::Result;
use serde_json::Value;
use crate::tools::get_tool_schema;
use crate::schemas::ToolLocation;

/// Executes tools either locally or remotely based on their location.
pub struct ToolExecutor {
    /// Base URL for remote envoy client (e.g., "http://localhost:8081").
    /// None means local-only mode — Client tools will error.
    envoy_url: Option<String>,
    /// Cached HTTP client for remote tool calls.
    client: reqwest::Client,
}

impl ToolExecutor {
    pub fn new(envoy_url: Option<String>) -> Self {
        Self {
            envoy_url,
            client: reqwest::Client::new(),
        }
    }

    /// Returns true if an envoy URL is configured.
    pub fn has_envoy(&self) -> bool {
        self.envoy_url.is_some()
    }

    /// Returns the configured envoy URL, if any.
    pub fn envoy_url(&self) -> Option<&str> {
        self.envoy_url.as_deref()
    }

    /// Execute a Server-location tool locally (synchronous).
    pub fn execute_server(&self, tool_name: &str, args: &Value) -> Result<String> {
        crate::tools::use_tool(tool_name, args)
    }

    /// Execute a tool with the configured strategy.
    pub async fn execute(
        &self,
        tool_name: &str,
        args: &Value,
        device_id: i64,
        device_key: &str,
    ) -> Result<String> {
        let schema = get_tool_schema(tool_name)?;

        match schema.location {
            ToolLocation::Server => {
                crate::tools::use_tool(tool_name, args)
            }
            ToolLocation::Client => {
                match &self.envoy_url {
                    Some(url) => {
                        self.execute_remote(url, device_id, device_key, tool_name, args).await
                    }
                    None => {
                        Err(anyhow::anyhow!(
                            "Tool '{}' requires client execution but no envoy URL is configured",
                            tool_name
                        ))
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
        args: &Value,
    ) -> Result<String> {
        let url = format!("{}/shared/execute", base_url);

        let request_body = serde_json::json!({
            "device_id": device_id,
            "device_key": device_key,
            "tool_name": tool_name,
            "arguments": args,
        });

        let response = self.client
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
