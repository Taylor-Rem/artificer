use tokio::sync::mpsc;
use axum::response::sse::Event;
use serde_json::Value;

/// A single SSE event ready to be sent to the client.
pub struct SseEvent {
    event_type: String,
    data: String,
}

impl SseEvent {
    pub fn to_sse(self) -> Result<Event, std::convert::Infallible> {
        Ok(Event::default()
            .event(self.event_type)
            .data(self.data))
    }
}

/// Sends structured events to the client over an SSE channel.
/// Created per-request by the handler, passed to the Orchestrator and specialists.
#[derive(Clone)]
pub struct EventSender {
    tx: mpsc::Sender<SseEvent>,
}

impl EventSender {
    pub fn new(tx: mpsc::Sender<SseEvent>) -> Self {
        Self { tx }
    }

    fn send(&self, event_type: &str, data: Value) {
        let _ = self.tx.try_send(SseEvent {
            event_type: event_type.to_string(),
            data: data.to_string(),
        });
    }

    pub fn task_switch(&self, from: &str, to: &str) {
        self.send("task_switch", serde_json::json!({
            "from": from,
            "to": to,
        }));
    }

    pub fn tool_call(&self, task: &str, tool: &str, args: Value) {
        self.send("tool_call", serde_json::json!({
            "task": task,
            "tool": tool,
            "args": args,
        }));
    }

    pub fn tool_result(&self, task: &str, tool: &str, result: String) {
        let truncated = result.len() > 5000;
        let display = if truncated {
            format!("{}... ({} chars total)", &result[..500], result.len())
        } else {
            result
        };

        self.send("tool_result", serde_json::json!({
            "task": task,
            "tool": tool,
            "result": display,
            "truncated": truncated,
        }));
    }

    pub fn stream_chunk(&self, content: String) {
        self.send("stream_chunk", serde_json::json!({
            "content": content,
        }));
    }

    pub fn error(&self, message: &str) {
        self.send("error", serde_json::json!({
            "message": message,
        }));
    }

    pub fn done(&self) {
        self.send("done", serde_json::json!({}));
    }
}
