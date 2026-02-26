# Artificer

A self-hosted AI infrastructure system built in Rust. Artificer runs on your own hardware and provides a persistent, autonomous agent capable of completing long-running tasks — not just answering questions.

## What It Is

Most AI interfaces are chatbots. You ask something, they respond, the interaction ends. Artificer is built around a different model: you give it a goal, and it works until the goal is achieved. It plans, delegates to specialists, evaluates its own progress, and keeps going until the work is actually done.

It runs entirely on your hardware. No data leaves your machine. Accessible from anywhere via Tailscale.

### Installation

Artificer is a distributed system with two components:

**Engine** — The server. Runs on your primary machine alongside your GPUs. Hosts the Orchestrator, manages GPU assignment, maintains the database, and runs background jobs.

**Envoy** — A lightweight client. Runs on any machine. Sends requests to the engine over HTTP and handles client-side tool execution (file operations run where the files are).

### The Orchestrator

The core of the engine. When a request comes in, the Orchestrator:

1. Creates a task record and defines success criteria
2. Sets a plan using its working memory tools
3. Delegates work to specialists
4. Evaluates results against the original goal
5. Continues until the goal is genuinely complete

The Orchestrator holds the goal and reasons about progress. It does not do the work itself — it directs specialists and synthesizes their output. This keeps its context clean across long tasks and allows it to work through complex multi-step problems without losing the thread.

### Specialists

Specialists are focused agents with their own reasoning loops. The Orchestrator delegates to them with specific instructions and gets back synthesized conclusions — not raw tool output.

**Interactive specialists** run on the primary GPU alongside the Orchestrator:
- `web_research` — Searches the web, fetches pages, synthesizes findings
- `file_smith` — Reads, writes, and manipulates files on the Envoy client

**Background specialists** run on a secondary GPU while the primary stays available:
- `title_generation` — Generates conversation and task titles
- `summarization` — Summarizes completed conversations and tasks
- `memory_extraction` — Extracts long-term facts, preferences, and context

### GPU Pool

Hardware is declared in `hardware.json` at the workspace root. The engine reads this at startup and manages GPU assignment at runtime.

```json
{
  "gpus": [
    {
      "id": "p40_primary",
      "url": "http://localhost:11435",
      "model": "qwen2.5:32b-instruct-q4_K_M",
      "role": "interactive"
    },
    {
      "id": "rtx3070_background",
      "url": "http://localhost:11434",
      "model": "qwen3:8b",
      "role": "background"
    }
  ]
}
```

Interactive GPUs are assigned to Orchestrator tasks. Background GPUs handle summarization, title generation, and memory extraction. Adding a second interactive GPU automatically enables two concurrent Orchestrator tasks — no code changes required.

### Database

SQLite with WAL mode. All state is local.

- **conversations** — Containers for message history
- **tasks** — One per user request. Tracks goal, plan, working memory, and status
- **messages** — Full message history linked to both conversation and task
- **local_data** — Long-term memory: facts, preferences, and context per device
- **background** — Job queue for post-completion processing
- **keywords** — Extracted from tasks for searchability

### Working Memory

The Orchestrator maintains task state across its entire execution:

- `set_plan` / `set_current_step` — Track where it is in the work
- `set_state` / `get_state` — Key-value store for counters, targets, accumulated results
- `checkpoint` — Persist progress and prune context for long tasks
- `mark_complete` — Explicit completion signal before the final response

Working memory is persisted to the database on every mutation. Context pruning rebuilds the prompt from task state rather than replaying history, so the Orchestrator can work through arbitrarily long tasks without degrading.

## Project Structure

```
artificer/
├── hardware.json          # GPU configuration
├── crates/
│   ├── engine/            # Server
│   │   └── src/
│   │       ├── api/       # HTTP handlers, SSE streaming, middleware
│   │       ├── orchestrator/  # Main loop, task state, tools, system prompt
│   │       ├── specialist/    # Registry and execution for all specialists
│   │       ├── background/    # Background job workers
│   │       └── pool.rs        # GPU pool and acquisition
│   ├── envoy/             # Client
│   │   └── src/
│   │       ├── client.rs  # HTTP communication with engine
│   │       ├── tools.rs   # Client-side tool execution (file operations)
│   │       └── ui.rs      # Terminal interface
│   └── shared/            # Types shared between engine and envoy
│       └── src/
│           ├── db/        # Database layer (all persistence logic)
│           └── tools/     # Tool definitions and toolbelts
```

## Hardware

Developed and tested on:
- **NVIDIA Tesla P40** (24GB VRAM) — Primary/interactive GPU, running `qwen2.5:32b-instruct-q4_K_M` via Ollama on port 11435
- **NVIDIA RTX 3070** (8GB VRAM) — Background GPU, running `qwen3:8b` via Ollama on port 11434

The P40 keeps models loaded for interactive response times. The 3070 unloads immediately after background tasks to minimize idle power draw.

Artificer is designed to scale horizontally with hardware. Adding a second P40 to `hardware.json` enables two concurrent Orchestrator sessions automatically.

## Prerequisites

- Rust (stable)
- Ollama with models pulled for each GPU
- Tailscale (for remote access)
- NVIDIA drivers and CUDA (for GPU power management)

## Running

```bash
# Start the engine
cd crates/engine
cargo run

# Start an envoy client (on any machine with access)
cd crates/envoy
cargo run
```

For development with hot reloading:

```bash
cargo watch -x run
```

## Configuration

The engine reads `hardware.json` from the workspace root. The envoy reads a config file specifying the engine URL and device key.

Device authentication is handled at the engine level. Each Envoy registers with a unique device key, scoping its memory and conversations to that device.

## Design Principles

**Tasks over conversations.** The fundamental unit is a task with a goal and a completion state, not an exchange of messages. Conversations are just containers.

**Delegation over monolith.** The Orchestrator reasons about what to do. Specialists reason about how to do it. Neither does the other's job.

**Hardware drives architecture.** GPU assignment, model selection, and power management are explicit decisions made in configuration, not hidden inside code.

**Persistence by default.** Working memory is written to the database on every mutation. A crash mid-task loses nothing except the current model call.

**Local first.** All data stays on your hardware. External network access only happens when a specialist explicitly makes a web request.
