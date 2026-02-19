# Artificer

A self-hosted, persistent AI infrastructure system built in Rust. Artificer runs locally on your hardware, routes requests through specialized task pipelines, and builds memory of your preferences and context over time.

## What it is

Most AI tooling assumes a centralized cloud service. Artificer is the opposite — a local agent system designed to run on your own GPUs, store everything in a local database, and get smarter about how you work the longer you use it.

It is not a wrapper around an API. It is infrastructure.

## Architecture

Artificer is a Cargo workspace with three crates:

- **engine** — the server. Handles API requests, task execution, background jobs, and the database.
- **envoy** — the client. A terminal interface that connects to the engine, handles streaming responses, and exposes client-side tools like file operations.
- **shared** — common types, the database layer, tool implementations, and the event system.

### Task Pipeline

Every user message flows through a router that decomposes it into a pipeline of specialized tasks:
```
User message → Router → [task_1, task_2, ...] → execute in sequence → streamed response
```

Current tasks:
- **Router** — analyzes requests and builds task pipelines
- **Chat** — conversational responses and memory recall
- **WebResearcher** — web search and article fetching via Brave Search API
- **Summarizer** — synthesizes content from previous pipeline steps
- **TitleGeneration** — background job, generates conversation titles
- **Summarization** — background job, summarizes completed conversations
- **MemoryExtraction** — background job, extracts facts and preferences from conversations

### Specialists

Tasks are assigned to specialists which determine the model and GPU used:

| Specialist | Model | GPU |
|------------|-------|-----|
| Quick | qwen2.5:3b-instruct-q4_K_M | 3070 (port 11434) |
| ToolCaller | qwen2.5:32b-instruct-q4_K_M | P40 (port 11435) |

Background jobs run on the 3070. Interactive tasks run on the P40.

### Memory System

After each conversation, background jobs extract facts, preferences, and context into a typed memory store. This gets injected into system prompts so the AI knows your environment and preferences over time without you repeating yourself.

Memory is typed:
- **fact** — objective, high-confidence information (OS, paths, project names)
- **preference** — how you like things done
- **context** — what you're currently working on

### Tool System

Tools are implemented as toolbelts using a declarative macro:
```rust
register_toolbelt! {
    WebSearch {
        description: "...",
        location: ToolLocation::Server,
        tools: {
            "search" => search {
                description: "...",
                params: ["query": "string" => "Search query"]
            }
        }
    }
}
```

Tool location determines execution:
- `ToolLocation::Server` — runs in the engine process
- `ToolLocation::Client` — forwarded via HTTP to the envoy client, so file operations run on the machine you're chatting from

Current toolbelts: **FileSmith**, **Archivist**, **WebSearch**, **Router**

### Authentication

Devices register with the engine and receive a secret key. Every request is authenticated. The envoy client self-heals — if credentials are rejected it re-registers automatically.

### Event Streaming

All responses stream via Server-Sent Events. The engine broadcasts task switches, tool calls, tool results, and content chunks in real time so the client can render progress as it happens.

## Setup

### Requirements

- Rust (latest stable)
- [Ollama](https://ollama.ai) with two instances running on separate GPUs
- [Brave Search API key](https://api.search.brave.com)

### Ollama Setup

Run two Ollama instances targeting different GPUs:
```bash
# GPU 0 (3070) — background tasks
CUDA_VISIBLE_DEVICES=0 OLLAMA_HOST=0.0.0.0:11434 ollama serve

# GPU 1 (P40) — interactive tasks  
CUDA_VISIBLE_DEVICES=1 OLLAMA_HOST=0.0.0.0:11435 ollama serve
```

Pull the required models on each instance:
```bash
ollama pull qwen2.5:3b-instruct-q4_K_M
ollama pull qwen2.5:32b-instruct-q4_K_M
```

### Configuration

Create a `.env` file in the workspace root:
```
BRAVE_API_KEY=your_key_here
```

### Running
```bash
# Start the engine
just dev-engine

# Start the envoy client (separate terminal)
just dev-envoy
```

Or without hot reload:
```bash
cargo run --bin artificer
cargo run --bin envoy
```

## Development
```bash
just build    # build everything
just test     # run tests
just clean    # clean and rebuild
```

The justfile has hot-reload commands for both crates that watch for changes and rebuild automatically.

## Status

Early but functional. The core pipeline — routing, task execution, streaming, memory, multi-device auth — works. Active development is focused on refining the task system and improving research capabilities.

Not ready for general use. Built for a specific hardware setup. Contributions welcome but expect sharp edges.
