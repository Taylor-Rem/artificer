# Task Tools

Task tools allow agents to manage their execution state. All agents (orchestrators and specialists) have access to task tools.

## Scope Rules

### Modification Tools (Own Task Only)
These tools operate on the agent's own task and auto-persist to the database:
- `task::set_agent_goal` — Set your interpretation of the goal
- `task::set_plan` — Set your execution plan (array of step strings)
- `task::set_current_step` — Mark which step you're on
- `task::mark_step_complete` — Complete the current step
- `task::mark_complete` — Mark the entire task complete

Agents can only modify their own task. Specialists cannot modify the parent task.

### Read Tools (Parent Context)
These tools are available to agents with a parent task:
- `task::get_parent_goal` — Read the orchestrator's goal
- `task::get_parent_plan` — Read the orchestrator's current plan

Returns an error if called by a primary orchestrator (no parent task).

## Auto-Persistence

All modification tools automatically persist to the database. Specifically:
- `set_agent_goal`, `set_plan`, `set_current_step`, `mark_step_complete` → calls `checkpoint_task`
- `mark_complete` → calls `complete_task`
- `mark_failed` → calls `fail_task`

No manual checkpoint calls are needed.

## Usage Examples

### Orchestrator Starting Work
```
1. User: "Analyze the codebase"
2. Orchestrator calls: task::set_agent_goal("Understand architecture and identify patterns")
3. Orchestrator calls: task::set_plan(["Read main files", "Identify modules", "Generate report"])
4. Orchestrator delegates to FileSmith
```

### Specialist Using Parent Context
```
1. FileSmith receives: "Read the main application files"
2. FileSmith calls: task::get_parent_goal()
   → Returns: "Understand architecture and identify patterns"
3. FileSmith now knows to focus on architecture during file reads
4. FileSmith calls: task::set_agent_goal("Extract architectural patterns from source files")
5. FileSmith executes and returns results
```

### Task Completion
```
1. Agent completes work
2. Agent calls: task::mark_complete()
3. Database updated with completion timestamp
4. Agent returns final response
```
