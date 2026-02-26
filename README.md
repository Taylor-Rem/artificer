# Artificer

A self-hosted AI infrastructure system built in Rust. Artificer runs on your own hardware and provides a persistent, autonomous agent capable of completing long-running tasks.

## Quick Start

### Prerequisites

- Rust (stable toolchain)
- Ollama with models loaded on configured GPUs
- (Optional) Tailscale for remote access

### Installation

1. Clone the repository
2. Create `hardware.json` in the workspace root:

```json
{
  "gpus": [
    {
      "id": "primary",
      "url": "http://localhost:11434",
      "model": "qwen2.5:32b-instruct-q4_K_M",
      "role": "interactive"
    }
  ]
}
```

3. Configure environment (optional):

```bash
cp .env.example .env
# Edit .env with your settings
```

### Running

**Start the engine:**
```bash
cd crates/engine
cargo run
```

**Start an envoy client:**
```bash
cd crates/envoy
cargo run
```

The envoy will automatically register with the engine on first run.

## Architecture

### Components

- **Engine**: Server component running on GPU machine
  - Orchestrator: Manages task execution and delegation
  - Specialists: Domain-specific agents (FileSmith, WebResearcher, Archivist)
  - Background Worker: Processes async jobs (title generation, etc.)
  - GPU Pool: Manages GPU assignment for concurrent tasks

- **Envoy**: Lightweight client for interacting with engine
  - Runs on any machine
  - Executes client-side tools (file operations)
  - Communicates with engine over HTTP

### How It Works

1. **User Request** → Envoy sends to engine
2. **Orchestrator** → Claims GPU, creates task, makes plan
3. **Specialists** → Orchestrator delegates work to specialists
4. **Tool Execution** → Specialists use tools to accomplish work
5. **Response** → Results streamed back to user via SSE

### GPU Management

Hardware is declared in `hardware.json`:

- **Interactive GPUs**: Run user-facing tasks (Orchestrator + Specialists)
- **Background GPUs**: Run async jobs (title generation, etc.)

The system scales horizontally — adding GPUs enables more concurrent tasks.

### Database

SQLite with WAL mode. All state persists locally:

- Conversations & messages
- Tasks & sub-tasks
- Device registry
- Background job queue

## Configuration

### hardware.json

```json
{
  "gpus": [
    {
      "id": "gpu_name",
      "url": "http://localhost:11434",
      "model": "model_name",
      "role": "interactive | background"
    }
  ]
}
```

### Environment Variables

- `ENVOY_URL`: Envoy tool server URL (default: `http://localhost:8081`)
- `BRAVE_API_KEY`: For web search functionality

## Development

### Project Structure

```
artificer/
├── crates/
│   ├── engine/          # Server
│   │   └── src/
│   │       ├── agent/       # Agent system
│   │       ├── api/         # HTTP handlers
│   │       ├── background/  # Background worker
│   │       └── pool/        # GPU & agent pools
│   ├── envoy/           # Client
│   └── shared/          # Shared code
│       └── src/
│           ├── db/      # Database layer
│           └── tools/   # Tool definitions
├── hardware.json        # GPU configuration
└── README.md
```

### Running Tests

```bash
cargo test --workspace
```

### Hot Reload Development

Using `cargo-watch` via `just`:

```bash
just dev-engine   # Engine with hot reload
just dev-envoy    # Envoy with hot reload
```

## API

See [API Documentation](crates/engine/src/api/API.md) for full endpoint details.

Key endpoints:
- `POST /chat` — Send messages (SSE streaming)
- `POST /devices/register` — Register new device
- `GET /status` — System status
- `GET /background/status` — Background job queue status

## Troubleshooting

**"All GPUs are currently busy"**
All interactive GPUs are in use. Wait for current tasks to complete, or add more GPUs to `hardware.json`.

**"Tool requires client execution but no envoy URL configured"**
Start an envoy client or set the `ENVOY_URL` environment variable.

**"Database error"**
Check file permissions on `memory.db`. Delete it and restart to recreate from scratch.

## License

[Your chosen license]
