# Background Worker System

The background worker processes asynchronous jobs that don't require an immediate user response.

## Architecture

- **Worker**: Long-running tokio task that polls the `background` table every 2 seconds
- **Jobs**: Database rows with method, arguments, status, and retry tracking
- **Agent Execution**: Jobs run through the same agent pool as interactive tasks

## Job Types

### Title Generation
- **Method**: `title_generation`
- **Agent**: TitleGenerator (OneTime mode)
- **Trigger**: Automatically queued after the first message in a new conversation
- **Purpose**: Generate a concise, descriptive conversation title via LLM

## Job Lifecycle

```
pending → running → completed
                 ↘ pending (retry if retries < max_retries)
                 ↘ failed  (when retries exhausted)
```

1. **Created**: Row inserted with `status = 'pending'`
2. **Running**: Worker claims job, sets `status = 'running'`
3. **Completed**: Job succeeds, `status = 'completed'`, result stored
4. **Retry**: Job fails but has retries left, reset to `status = 'pending'`
5. **Failed**: Retries exhausted, `status = 'failed'`, fallback applied

## Configuration

- Poll interval: 2 seconds (configured in `main.rs`)
- Max retries: Stored per-job in `background.max_retries`
- GPU: Uses background GPU handle from `GpuPool`
- Cleanup: Completed/failed jobs older than 7 days are deleted (runs every 24h)
- Drain timeout: 30 seconds on graceful shutdown

## Adding New Job Types

1. Define an agent in `implementations/specialists.rs`
2. Add a `title_generation.rs`-style executor module in `background/`
3. Add a match arm in `Worker::process_next_job()`
4. Queue jobs via `db.create_job()`

```rust
db.create_job(
    device_id,
    "my_job_type",
    &serde_json::json!({ "arg": "value" }),
    0, // priority (higher = runs first)
)?;
```

## Monitoring

`GET /background/status` returns current queue counts:

```json
{
  "pending": 0,
  "running": 1,
  "failed": 0,
  "completed": 42
}
```
