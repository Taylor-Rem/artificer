# Artificer

A self-hosted agent system in Rust: give it a goal, and an orchestrator agent
plans, delegates to specialist agents, records its progress in SQLite, and
keeps going until it marks the task complete. Models run on your own GPUs.

- **Who it is for:** one person or a small team with a few local GPUs who wants
  long-running agent work done on their own hardware. Nothing leaves the
  machine except web searches a specialist makes on purpose.
- **Status:** built 2026-01-26 to 2026-03-11 by Taylor Remund; **dormant since
  2026-03-11, not maintained.**
- **How it was built:** with AI coding tools, Taylor Remund as architect and
  reviewer.
- **Stack:** Rust (edition 2024) · Tokio · Axum · SQLite (rusqlite, WAL) ·
  reqwest · Ollama for models · Brave Search API for web research.
- **Tests:** none yet. Four placeholder `#[tokio::test]` functions exist, all
  `#[ignore]` with TODO bodies.
- **Licence:** MIT (see [LICENSE](LICENSE)).

## What it does

- Accepts a request from a client over HTTP and streams the work back as
  server-sent events: reasoning, tool calls, tool results, the answer.
- Runs an **Orchestrator** that sets a goal and a plan, tracks steps and notes,
  and delegates. It does no file or web work itself.
- Delegates to three **specialists**, each with its own loop (at most 25
  iterations) and toolbelt:
  - `FileSmith`: 15 file tools, executed on the client machine through envoy
  - `WebResearcher`: 3 tools over the Brave Search API
  - `Archivist`: 6 tools over the conversation database
- Assigns GPUs from `hardware.json`: interactive GPUs for user requests,
  background GPUs for housekeeping (conversation titles). If every
  interactive GPU is busy, the request gets HTTP 503 `resource_busy`.
- Persists everything in SQLite: devices, conversations, tasks (goal, plan,
  working memory, status), messages, background jobs and a per-iteration
  execution trace.

## Architecture

One Cargo workspace with three crates. **engine** (binary `artificer`) is the
server: an Axum API with five routes, the agent runtime, the GPU pool and a
background worker. **envoy** (binary `envoy`) is the terminal client; it
registers the machine as a device, sends requests, and runs a small tool
server on port 8081 so file operations happen where the files are. **shared**
is the library both use: the SQLite layer, the tool registry and toolbelts,
and the executor that runs a tool in-process or forwards it to envoy. Task
state is kept outside the transcript: the orchestrator's system prompt is
rebuilt from it every iteration, and each specialist iteration is built from
scratch from its current state, so a specialist never replays its history.

```
artificer/
├── hardware.json              # GPUs: endpoint, model, role
├── .env.example               # BRAVE_API_KEY, ENVOY_URL
├── justfile                   # dev-engine, dev-envoy, build, test
└── crates/
    ├── engine/src/
    │   ├── api/               # routes, handlers, SSE events, middleware
    │   ├── agent/             # agents, execution loop, task state, tools, prompts
    │   ├── pool/              # GPU pool and agent pool
    │   └── background/        # background job worker
    ├── envoy/src/             # client: config, HTTP client, terminal UI, tool server
    └── shared/src/
        ├── db/                # SQLite schema and persistence
        ├── tools/             # tool registry and toolbelts
        └── executor/          # local vs. client tool execution
```

## Running it

Not re-verified since the last commit (2026-03-11). What the repository
provides:

**Prerequisites:** Rust (stable, edition 2024); Ollama serving the models
named in `hardware.json`; a Brave Search API key for web research. The
author's setup also used Tailscale for remote access.

1. Edit `hardware.json` in the workspace root: one entry per GPU with an
   `id`, the Ollama `url`, the `model`, and a `role` of `interactive` or
   `background`.
2. Copy `.env.example` to `.env` and set `BRAVE_API_KEY`. `ENVOY_URL`
   defaults to `http://localhost:8081`.
3. Start the engine from the workspace root (it creates `memory.db` in the
   current directory and listens on `0.0.0.0:8080`):
   ```bash
   cargo run --bin artificer
   ```
4. Start a client (on the same or another machine):
   ```bash
   cargo run --bin envoy                         # interactive chat
   cargo run --bin envoy -- "your message"       # one message
   cargo run --bin envoy -- config set server http://<engine-host>:8080
   ```
   envoy keeps its settings in `~/.config/envoy/config.json` and registers
   the machine as a device on first run.

With [just](https://github.com/casey/just) and `cargo-watch`: `just
dev-engine`, `just dev-envoy` (hot reload), `just build`, `just test`.

Note: `SERVER_ADDR` in `.env.example` is not read; the address is fixed in
`crates/engine/src/api/server.rs`.

## Hardware it was developed on

- NVIDIA Tesla P40 (24 GB), interactive, `qwen2.5:32b-instruct-q4_K_M` via
  Ollama on port 11435
- NVIDIA RTX 3070 (8 GB), background, `qwen3:8b` via Ollama on port 11434

Adding another interactive GPU to `hardware.json` lets two orchestrator tasks
run at once without code changes.

## Design principles

**Tasks over conversations.** The unit of work is a task with a goal and a
completion state; conversations are containers.

**Delegation over monolith.** The orchestrator decides what to do;
specialists decide how. Neither does the other's job.

**Hardware drives architecture.** GPU assignment and model choice are
configuration, not code.

**Persistence by default.** Task state is written to the database after each
round of tool calls, so a crash loses at most the round in progress.

**Local first.** Data stays on your hardware; the network is used only when a
specialist makes a web request.

## Licence

MIT. See [LICENSE](LICENSE).
