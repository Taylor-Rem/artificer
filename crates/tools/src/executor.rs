use anyhow::Result;
use serde_json::Value;

/// Determines how a tool should be executed at runtime
pub enum ToolExecutor {
    /// Execute tool directly in the current process
    Local,
    /// Execute tool via HTTP request to a remote envoy client
    Remote {
        base_url: String,
        device_id: i64,
        device_key: String,
    },
}

impl ToolExecutor {
    /// Create a local executor (runs tools in current process)
    pub fn local() -> Self {
        Self::Local
    }

    /// Create a remote executor (sends tool calls to envoy via HTTP)
    pub fn remote(base_url: String, device_id: i64, device_key: String) -> Self {
        Self::Remote { base_url, device_id, device_key }
    }

    /// Execute a tool with the configured strategy
    pub async fn execute(&self, tool_name: &str, args: &Value) -> Result<String> {
        match self {
            ToolExecutor::Local => {
                // Direct local execution
                crate::registry::use_tool(tool_name, args)
            }
            ToolExecutor::Remote { base_url, device_id, device_key } => {
                // Remote execution via HTTP
                self.execute_remote(base_url, *device_id, device_key, tool_name, args).await
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
        let client = reqwest::Client::new();
        let url = format!("{}/tools/execute", base_url);

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