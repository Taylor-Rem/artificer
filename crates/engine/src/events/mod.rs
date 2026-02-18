use tokio::sync::broadcast;
use once_cell::sync::Lazy;
pub use artificer_shared::events::ChatEvent;

// Global event broadcaster
static EVENT_CHANNELS: Lazy<std::sync::Mutex<std::collections::HashMap<String, broadcast::Sender<ChatEvent>>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Create a new event channel for a conversation/request
pub fn create_channel(id: String) -> broadcast::Receiver<ChatEvent> {
    let mut channels = EVENT_CHANNELS.lock().unwrap();

    // Create new channel with buffer of 100 events
    let (tx, rx) = broadcast::channel(100);
    channels.insert(id.clone(), tx);

    rx
}

/// Send an event to a specific channel
pub fn send_event(id: &str, event: ChatEvent) {
    let channels = EVENT_CHANNELS.lock().unwrap();
    if let Some(tx) = channels.get(id) {
        let _ = tx.send(event); // Ignore if no receivers
    }
}

/// Clean up a channel when done
pub fn cleanup_channel(id: &str) {
    let mut channels = EVENT_CHANNELS.lock().unwrap();
    channels.remove(id);
}

/// Helper to send events from anywhere
#[derive(Clone)]
pub struct EventSender {
    request_id: String,
}

impl EventSender {
    pub fn new(request_id: String) -> Self {
        Self { request_id }
    }

    pub fn task_switch(&self, from: &str, to: &str) {
        send_event(&self.request_id, ChatEvent::TaskSwitch {
            from: from.to_string(),
            to: to.to_string(),
        });
    }

    pub fn tool_call(&self, task: &str, tool: &str, args: serde_json::Value) {
        send_event(&self.request_id, ChatEvent::ToolCall {
            task: task.to_string(),
            tool: tool.to_string(),
            args,
        });
    }

    pub fn tool_result(&self, task: &str, tool: &str, result: String) {
        let truncated = result.len() > 500;
        let display_result = if truncated {
            format!("{}... ({} chars total)", &result[..500], result.len())
        } else {
            result.clone()
        };

        send_event(&self.request_id, ChatEvent::ToolResult {
            task: task.to_string(),
            tool: tool.to_string(),
            result: display_result,
            truncated,
        });
    }

    pub fn stream_chunk(&self, content: String) {
        send_event(&self.request_id, ChatEvent::StreamChunk { content });
    }

    pub fn complete(&self, conversation_id: u64) {
        send_event(&self.request_id, ChatEvent::Done { conversation_id });
        cleanup_channel(&self.request_id);
    }

    pub fn error(&self, message: String) {
        send_event(&self.request_id, ChatEvent::Error { message });
        cleanup_channel(&self.request_id);
    }
}