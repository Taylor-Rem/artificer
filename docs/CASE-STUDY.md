# Artificer: a case study

Built 2026-01-26 to 2026-03-11 by Taylor Remund. **Dormant since 2026-03-11.**
Every number below was measured on 2026-10-09 at commit `f4e3bf3`; the
commands are listed at the end.

## What it is, and who it is for

Artificer is a self-hosted agent system written in Rust. You give it a goal.
An orchestrator agent records the goal, writes a plan, delegates the work to
specialist agents, tracks its progress in SQLite, and keeps going until it
marks the task complete. Models run on your own GPUs behind local model
servers (Ollama's `/api/chat`, one endpoint per GPU), and a small terminal
client, envoy, talks to the engine over HTTP from any machine.

It is for one person or a small team who owns a few GPUs and wants
long-running agent work done locally. Nothing leaves the machine except the
web searches a specialist makes on purpose.

## The problem

A chat interface stops when the answer stops. An agent loop that keeps going
runs into two practical limits on home hardware. The context window fills
with old tool output, so a long task degrades as it runs. And two or three
GPUs have to be shared between the long-running job and small housekeeping
work such as naming a conversation. Artificer's design answers both: keep the
state of a task outside the transcript, and treat each GPU as a resource that
is held and released explicitly.

## Architecture

One Cargo workspace (edition 2024), three crates, 6,172 lines of Rust in 46
files (5,167 excluding blank and comment-only lines):

| crate | kind | lines | role |
|---|---|---|---|
| `engine` | binary `artificer` | 3,794 | HTTP API, agent runtime, GPU pool, background worker |
| `envoy` | binary `envoy` | 483 | terminal client, plus a small tool server for file operations |
| `shared` | library | 1,895 | SQLite layer, tool registry and toolbelts, tool executor |

**The orchestrator.** A `POST /chat` request is authenticated by device key,
attached to a conversation, given an interactive GPU, and handed to the
orchestrator on a spawned task; reasoning, tool calls and results stream back
to the client as server-sent events. The orchestrator has task tools
(`task::set_agent_goal`, `task::set_plan`, `task::set_current_step`,
`task::set_note`, `task::mark_complete` and others) and three delegation
tools. It does not touch files or the web itself. It delegates to
specialists: **FileSmith** (15 file tools, executed on the client machine),
**WebResearcher** (3 tools over the Brave Search API) and **Archivist**
(6 tools over the conversation database). A specialist runs as a child task
on the same GPU as its parent, is capped at 25 iterations, and hands back a
composed answer through its own `response::` tools rather than raw tool
output.

**The GPU pool.** `hardware.json` lists each GPU with an endpoint URL, a model
and a role: `interactive` or `background`. `GpuPool` keeps the ids of busy
GPUs in a `Mutex<HashSet<String>>` and exposes `acquire_interactive()`,
`acquire_background()` and `release()`. A chat request that finds no free
interactive GPU gets HTTP 503 with `"type": "resource_busy"`. A background
worker polls a job table every two seconds, takes a background GPU when one is
free, and generates conversation titles; on shutdown the engine drains that
queue before exiting.

**Task persistence.** SQLite in WAL mode with six tables: devices,
conversations, tasks, messages, background jobs and execution traces. A task
row holds the goal, the plan, a working-memory JSON blob and a status, and
child tasks point at their parent. Every LLM iteration also writes an
execution-trace row: a prompt preview, the tool calls, their results, a
classification of what the iteration did, and how long the model call took.

## Four design decisions and their trade-offs

**1. Task state lives outside the transcript.** The orchestrator's system
prompt is rebuilt on every iteration from the task state, rendered as XML:
goal, phase, plan steps with their status, an iteration counter, and up to 20
notes ordered by importance (the least important note is evicted when a
twenty-first arrives). Specialists go further: each iteration is built from
scratch as exactly two messages, the system prompt and the request plus the
current execution state, so a specialist never replays its own history.
State changes mark the task dirty, and the row is written after each round of
tool calls. *Trade-off:* the model knows only what the state captures, and the
20-note cap forces it to choose what to forget. The orchestrator still carries
the conversation history in its message list, so the no-growth property holds
fully for specialists, not for the orchestrator.

**2. GPUs are acquired and released explicitly.** `GpuHandle` deliberately does
not release itself when dropped; the code comments say so, so that every
release is logged and intentional. Requests that cannot get a GPU fail fast
with a 503 instead of waiting in a queue. *Trade-off:* every exit path has to
remember to call `release()`. In the background worker, some early error
returns skip it, which would leave a GPU marked busy until restart. A guard
type that releases on drop and still logs would keep the intent and close
that gap. Failing fast keeps latency predictable, but it pushes retrying onto
the client.

**3. One tool registry, two places to run.** Tools are declared once in the
shared crate with a `register_toolbelt!` macro that generates the handlers
and the JSON schemas the model sees. Each toolbelt declares where it runs:
`Server` (web search, archive queries) or `Client` (file operations).
`ToolExecutor` runs server tools in-process and forwards client tools over
HTTP to the envoy's tool server, which accepts only calls carrying its own
device id and key. Engine and client compile against the same definitions, so
schema and implementation cannot drift apart. *Trade-off:* the client has to
run a reachable tool server, and every file operation costs a network round
trip.

**4. Agents are declared, not assembled by hand.** A `define_agents!` macro
(`macro_rules!`, not a procedural macro) generates an `AgentType` enum and
builds each agent's tool list from toolbelt prefixes and a few flags (task
tools, delegation tools, specialist control tools). Adding a specialist is one
block plus a prompt file. *Trade-off:* the set of agents is fixed at compile
time, and the flags are plain booleans: an invalid combination of role and
execution mode is caught at run time by the executor, not by the type system.

## How it was built

Artificer was built with AI coding tools, in the way Taylor works generally:
engineer first, with AI as the leverage. He set the architecture and owned
the review and verification; AI agents wrote much of the implementation under
his direction. The history shows the pace: 105 commits on `main` (63
excluding merges) between 2026-01-26 and 2026-03-11, plus five side branches,
among them `redb-impl`, `home-experiment` and `vec-tool-experiment`.

## Status

Built 2026-01-26 to 2026-03-11; no commits since, on any branch. It is not
maintained.

There are no automated tests. The engine has four `#[tokio::test]` functions,
all marked `#[ignore]` with bodies that are still TODO comments; they name
what should be tested first: task-state changes, server-side and client-side
tool routing, and validation of required parameters.

What would come next, in order: real tests for those four cases; a releasing
guard for GPU handles; a short queue in front of the 503; and an option to
rebuild the orchestrator's context from task state alone, as the specialists
already do.

## Measurements

Taken in a clean clone at `f4e3bf3` on 2026-10-09:

```
git rev-list --count HEAD                                  # 105
git rev-list --count --no-merges HEAD                      # 63
git log --reverse --format=%ad --date=short | head -1      # 2026-01-26
git log --all -1 --format=%ad --date=short                 # 2026-03-11
find crates -name '*.rs' | xargs wc -l | tail -1           # 6172 total
find crates -name '*.rs' | wc -l                           # 46
find crates -name '*.rs' | xargs cat | grep -v '^\s*$' | grep -v '^\s*//' | wc -l   # 5167
find crates/<crate> -name '*.rs' | xargs cat | wc -l       # engine 3794, envoy 483, shared 1895
grep -rn '#\[test\]' --include=*.rs crates | wc -l         # 0
grep -rn '#\[tokio::test\]' --include=*.rs crates | wc -l  # 4 (all #[ignore])
grep -c 'route(' crates/engine/src/api/routes.rs           # 5
grep -c 'CREATE TABLE' crates/shared/src/db/schema.rs      # 6
```
