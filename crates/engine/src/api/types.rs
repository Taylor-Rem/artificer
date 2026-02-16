use serde::{Deserialize, Serialize};

// Chat endpoint
#[derive(Deserialize)]
pub struct ChatRequest {
    pub device_id: i64,
    pub conversation_id: Option<u64>,
    pub message: String,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub conversation_id: u64,
    pub content: String,
}

// Device registration
#[derive(Deserialize)]
pub struct RegisterDeviceRequest {
    pub device_name: String,
}

#[derive(Serialize)]
pub struct RegisterDeviceResponse {
    pub device_id: i64,
}

// Conversation listing
#[derive(Serialize)]
pub struct ConversationInfo {
    pub id: u64,
    pub title: Option<String>,
    pub created: i64,
    pub last_accessed: i64,
}

#[derive(Serialize)]
pub struct ListConversationsResponse {
    pub conversations: Vec<ConversationInfo>,
}