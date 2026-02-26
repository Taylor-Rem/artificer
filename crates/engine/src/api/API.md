# Artificer API Documentation

Base URL: `http://localhost:8080` (or configured server address)

## Authentication

All endpoints (except `/devices/register`) require device authentication via `device_id` and `device_key` in the request body.

## Endpoints

### POST /chat

Primary endpoint for sending messages to Artificer.

**Request:**
```json
{
  "device_id": 123,
  "device_key": "uuid-device-key",
  "conversation_id": 456,
  "message": "Your message here"
}
```

`conversation_id` is optional — omit to start a new conversation.

**Response:** Server-Sent Events (SSE) stream

Event types:
- `task_switch`: Agent transitioning between tasks
- `tool_call`: Agent calling a tool
- `tool_result`: Tool execution result
- `stream_chunk`: Partial response content (streaming)
- `done`: Request complete
- `error`: Error occurred

**Example SSE events:**
```
event: tool_call
data: {"type":"tool_call","task":"task_1","tool":"FileSmith::read_file","args":{"path":"config.json"}}

event: tool_result
data: {"type":"tool_result","task":"task_1","tool":"FileSmith::read_file","result":"...","truncated":false}

event: stream_chunk
data: {"type":"stream_chunk","content":"Based on the config..."}

event: done
data: {"type":"done","conversation_id":456}
```

### POST /devices/register

Register a new device to get credentials.

**Request:**
```json
{
  "device_name": "my-laptop"
}
```

**Response:**
```json
{
  "device_id": 123,
  "device_key": "uuid-device-key"
}
```

Store these credentials — you'll need them for all subsequent requests. Re-registering the same `device_name` rotates the key.

### POST /devices/verify

Verify stored credentials are still valid.

**Request:**
```json
{
  "device_id": 123,
  "device_key": "uuid-device-key"
}
```

**Response:**
- `200 OK`: Credentials valid
- `401 Unauthorized`: Credentials invalid

### GET /status

Check server and GPU status.

**Response:**
```json
{
  "status": "ok",
  "gpus": [
    {
      "id": "p40",
      "url": "http://localhost:11435",
      "model": "qwen2.5:32b-instruct-q4_K_M",
      "role": "interactive",
      "busy": false
    }
  ]
}
```

### GET /background/status

Check background job queue status.

**Response:**
```json
{
  "pending": 2,
  "running": 1,
  "failed": 0,
  "completed": 47
}
```

## Error Responses

All errors follow this format:

```json
{
  "error": "Human-readable error message",
  "type": "error_type",
  "field": "field_name"
}
```

`field` is only present for `invalid_request` errors.

Error types:
- `authentication`: Invalid or deactivated credentials
- `not_found`: Resource not found
- `invalid_request`: Bad request data (see `field` for which field)
- `resource_busy`: All GPUs busy, retry later
- `internal_error`: Server-side error

## Request Validation

`/chat` enforces:
- `message` cannot be empty
- `message` cannot exceed 50,000 characters
- `device_key` cannot be empty

## Streaming

The `/chat` endpoint always uses Server-Sent Events. To consume:

**curl:**
```bash
curl -N -X POST http://localhost:8080/chat \
  -H "Content-Type: application/json" \
  -d '{"device_id":123,"device_key":"...","message":"Hello"}'
```

**JavaScript:**
```javascript
const res = await fetch('/chat', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ device_id: 123, device_key: '...', message: 'Hello' }),
});

const reader = res.body.getReader();
const decoder = new TextDecoder();

while (true) {
  const { done, value } = await reader.read();
  if (done) break;
  console.log(decoder.decode(value));
}
```
