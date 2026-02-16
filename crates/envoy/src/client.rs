use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ChatRequest {
    pub device_id: i64,
    pub device_key: String,
    pub conversation_id: Option<u64>,
    pub message: String,
}
#[derive(Deserialize, Debug)]
pub struct RegisterDeviceResponse {
    pub device_id: i64,
    pub device_key: String
}
#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    pub conversation_id: u64,
    pub content: String,
}
#[derive(Clone)]
pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
}

impl ApiClient {
    pub fn new(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
        }
    }

    pub async fn chat(
        &self,
        device_id: i64,
        device_key: String,
        conversation_id: Option<u64>,
        message: String,
    ) -> Result<ChatResponse> {
        let url = format!("{}/chat", self.base_url);

        let request = ChatRequest {
            device_id,
            device_key,
            conversation_id,
            message,
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        // Check for device not found / auth error
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::NOT_FOUND
            || response.status() == reqwest::StatusCode::BAD_REQUEST {
            return Err(anyhow::anyhow!("Device not found - please re-register"));
        }

        let response = response.json::<ChatResponse>().await?;
        Ok(response)
    }

    pub async fn register_device(&self, device_name: String) -> Result<(i64, String)> {
        let url = format!("{}/devices/register", self.base_url);

        let response = self.client
            .post(&url)
            .json(&serde_json::json!({ "device_name": device_name }))
            .send()
            .await?
            .json::<RegisterDeviceResponse>()
            .await?;

        Ok((response.device_id, response.device_key))
    }
    pub async fn queue_summarization(
        &self,
        device_id: i64,
        conversation_id: u64,
    ) -> Result<u64> {
        let url = format!("{}/jobs/summarize", self.base_url);

        let response = self.client
            .post(&url)
            .json(&serde_json::json!({
                "device_id": device_id,
                "conversation_id": conversation_id
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let job_id = response["job_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Invalid job_id in response"))?;

        Ok(job_id)
    }

    pub async fn queue_memory_extraction(
        &self,
        device_id: i64,
        conversation_id: u64,
    ) -> Result<u64> {
        let url = format!("{}/jobs/extract_memory", self.base_url);

        let response = self.client
            .post(&url)
            .json(&serde_json::json!({
                "device_id": device_id,
                "conversation_id": conversation_id
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        let job_id = response["job_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Invalid job_id in response"))?;

        Ok(job_id)
    }
}