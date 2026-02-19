use anyhow::Result;
use serde::{Deserialize, Serialize};
use futures_util::StreamExt;
use artificer_shared::events::ChatEvent;
#[derive(Serialize)]
pub struct ChatRequest {
    pub device_id: i64,
    pub device_key: String,
    pub conversation_id: Option<u64>,
    pub message: String,
    stream: Option<bool>,
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
            stream: None
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

    pub async fn chat_stream(
        &self,
        device_id: i64,
        device_key: String,
        conversation_id: Option<u64>,
        message: String,
        mut event_handler: impl FnMut(ChatEvent),
    ) -> Result<u64> {
        let url = format!("{}/chat", self.base_url);

        let request = ChatRequest {
            device_id,
            device_key,
            conversation_id,
            message,
            stream: Some(true),
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Request failed: {}", response.status()));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut final_conv_id = 0;

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            buffer.extend_from_slice(&bytes);

            // Process complete lines
            while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=newline_pos).collect();
                let line = String::from_utf8_lossy(&line);

                // SSE format: "data: {json}\n"
                if let Some(data) = line.strip_prefix("data: ") {
                    let data = data.trim();
                    if data.is_empty() {
                        continue;
                    }

                    if let Ok(event) = serde_json::from_str::<ChatEvent>(data) {
                        if let ChatEvent::Done { conversation_id } = &event {
                            final_conv_id = *conversation_id;
                        }
                        event_handler(event);
                    }
                }
            }
        }

        Ok(final_conv_id)
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
        device_key: String,
        conversation_id: u64,
    ) -> Result<u64> {
        let url = format!("{}/jobs/summarize", self.base_url);

        let response = self.client
            .post(&url)
            .json(&serde_json::json!({
                "device_id": device_id,
                "device_key": device_key,
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
        device_key: String,
        conversation_id: u64,
    ) -> Result<u64> {
        let url = format!("{}/jobs/extract_memory", self.base_url);

        let response = self.client
            .post(&url)
            .json(&serde_json::json!({
                "device_id": device_id,
                "device_key": device_key,
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