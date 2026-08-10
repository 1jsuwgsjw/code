<!-- CTF|认证 -->

# T-003 Project AGENT Architecture

## Architectural judgment

The repository already contains most required runtime primitives: config-backed agent roles,
subagent threads, extension-contributed tools and context, remote executor filesystems, MCP tool
filters, thread persistence, and two-phase memory processing. The new feature should compose those
primitives rather than add another orchestration engine.

The project-specific feature belongs primarily in two new crates:

1. `codex-project-agents` (`codex-rs/project-agents`): storage-neutral file models, validation,
   project-root registry operations, fixed result types, and bounded file layout helpers.
2. `codex-project-agents-extension` (`codex-rs/ext/project-agents`): thread lifecycle integration,
   dynamic AGENT tools, isolated worker spawning, `.x` tool execution, task summaries, memory and
   improvement queues, and maintenance status.

Small generic host changes may be made in existing crates only where the repository lacks a reusable
capability. The known generic gaps are project-root resolution as a public API, core-wide tool
visibility filtering, and forwarding an extension-owned maintenance notice to product surfaces.

## Reused repository primitives

| Requirement | Reused primitive |
| --- | --- |
| Project root on local or remote executors | `codex-config` marker semantics plus `ExecutorFileSystem` |
| Long-lived role definition | `AgentRoleConfig` and role config-layer application |
| Model/reasoning/provider override | Existing `ConfigToml` role files and `apply_role_to_config` behavior |
| Agent worker thread | `ThreadManager`, `StartThreadOptions`, `CodexThread`, `AgentStatus` |
| Fixed final payload | `Op::UserInput.final_output_json_schema` |
| Main-model AGENT tools | `ToolContributor`, `ToolExecutor`, `ExtensionToolAdapter` |
| `.x` tool dispatch | Extension tool executors plus selected `Environment` process/filesystem backends |
| MCP references | `McpConnectionManager` and MCP `enabled_tools`/`disabled_tools` |
| Runtime history | `thread-store`, rollout persistence, state DB, and agent graph metadata |
| Memory processing patterns | `memories/write` phase extraction/consolidation and `ext/memories` contributors |
| CLI wiring | Existing `mcp_cmd.rs`/`plugin_cmd.rs` module pattern |
| TUI rendering/tests | App-server notifications, TUI history/status components, `insta` snapshots |

## Runtime ownership

```text
codex / codex exec
  -> app-server + ThreadManager
  -> ProjectAgentsExtension::on_thread_start
       -> resolve project root through selected ExecutorFileSystem
       -> bootstrap/load AGENT/registry.toml
       -> cache validated catalog in thread ExtensionData
  -> ProjectAgentsExtension::tools
       -> expose one model tool for each enabled specialist AGENT
  -> main model calls agent.<agent_id>
       -> executor builds an isolated Config from the definition
       -> host spawns a worker thread
       -> worker receives bounded definition, constraints, task, and accepted memory
       -> worker sees only its registered tools
       -> worker final response is constrained by the fixed JSON schema
       -> result/task summary/candidates are persisted
       -> structured result returns to the main model
```

Project AGENT workers remain host-owned threads. They do not own the registry, cannot edit their
definition, and cannot install or enable tools. Their only write path into long-lived behavior is the
candidate/proposal fields in their validated result.

## On-disk layout

```text
AGENT/
  registry.toml
  agents/
    <agent-id>/
      agent.toml
      constraints.md
      tools/
        <tool-id>.x
      memory/
        facts/
        experience/
        bottlenecks/
        index.toml
      tasks/
        current.json
        history/
      proposals/
        pending/
        accepted/
        rejected/
```

All paths stored by the registry are relative to `AGENT/`. Absolute paths and parent traversal are
rejected. Identifiers use a conservative lowercase ASCII slug. Every structured file carries a
`schema_version`.

### `registry.toml`

```toml
schema_version = 1

[agents.query]
path = "agents/query/agent.toml"
enabled = true
```

The registry is the source of discovery. Directory enumeration does not implicitly activate an
AGENT.

### `agent.toml`

```toml
schema_version = 1
id = "query"
description = "Performs bounded repository queries and returns cited findings."
constraints_file = "constraints.md"
model = "gpt-5.6-luna"
model_reasoning_effort = "medium"
model_provider = "default"
tools = ["tools/search.x"]
memory_max_items = 32
memory_max_tokens = 4000
```

`description` is both the main AGENT's selection guidance and the first boundary check. Detailed
domain rules and the fixed out-of-scope behavior live in `constraints.md`.

### `.x` manifest

```toml
schema_version = 1
id = "search"
description = "Runs the project's bounded search helper."
kind = "command"
program = "tools/search.py"
timeout_ms = 30000
input_schema = "tools/search.schema.json"
```

Initial kinds:

- `native`: references a host-native `ToolName`.
- `command`: references an executable/script stored separately from the manifest.
- `mcp`: references an already configured MCP server/tool.

The manifest never stores a script body, provider secret, or unbounded output policy.

## Fixed result model

The wire/result enum is exhaustive:

```text
completed
rejected_out_of_scope
blocked_missing_tool
failed
```

The JSON schema requires every top-level field from the confirmed protocol. Result parsing failure is
a host-side `failed` result and never becomes accepted memory. The worker's free-form final text is
not trusted as a control signal.

## Tool isolation

MCP already has per-server filters, but there is no generic filter for native and extension tools.
Add a reusable extension-level tool visibility policy rather than a project-agent branch throughout
core tool planning.

Proposed host contract:

```rust
pub trait ToolVisibilityContributor: Send + Sync {
    fn visibility(
        &self,
        session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> ToolVisibilityPolicy;
}
```

`ToolVisibilityPolicy` supports an optional allow set and a deny set of `ToolName`. Core combines all
policies by intersection for allow sets and union for deny sets, then filters planned runtimes and
hosted specs before constructing `ToolRegistry`. Because filtered tools never enter the registry, the
same operation enforces both model visibility and dispatch.

Root/user threads receive no project-agent policy. A worker thread gets a policy from immutable
`ProjectAgentThreadContext` inserted through `StartThreadOptions.thread_extension_init`.

## Context construction

Always loaded for a worker:

- stable identity and description;
- `constraints.md` under a hard byte/token cap;
- fixed result contract;
- current unit task;
- registered tool schemas.

Retrieved on demand:

- at most `memory_max_items` accepted memories;
- at most `memory_max_tokens` total memory context;
- only entries selected for the current task.

Never injected:

- raw task logs;
- entire history directories;
- pending or rejected memory candidates;
- unrelated AGENT definitions;
- tools not present in the worker's allowlist.

## Mutation and maintenance

Normal mode may append a bounded task summary and create immutable candidate/proposal files. It may
not rewrite definitions, constraints, tool manifests, or accepted memory.

Manual maintenance:

1. acquire an AGENT-level maintenance lock;
2. snapshot pending items;
3. validate evidence, schema, paths, duplicates, and capacity;
4. record `accepted` or `rejected` decisions;
5. atomically write accepted changes;
6. increment the definition/catalog revision;
7. reload the catalog for subsequent turns;
8. retain an attributable maintenance report.

The TUI displays only a non-blocking count/reminder. It never enters maintenance automatically.

## Product surfaces

### CLI

Keep `cli/src/main.rs` changes to module registration, enum wiring, and dispatch. Put behavior in a new
`cli/src/agent_cmd.rs`.

Initial commands:

```text
codex agents list [--json]
codex agents show <id> [--json]
codex agents create <id> --description <text>
codex agents disable <id>
codex agents maintain [<id>] [--dry-run]
```

### TUI

The app-server/extension owns filesystem inspection so remote executor layouts remain correct. A
typed app-server notification carries project root, pending count, and catalog revision. TUI renders
the reminder and exposes a manual maintenance action. UI changes require snapshots.

## Staged implementation

### Slice 1A - Foundation

Target: under 500 changed logical lines where practical.

- add `codex-project-agents` crate;
- define registry/agent/tool/result types and validation;
- reuse `codex_file_system::find_nearest_ancestor_with_markers` for `PathUri` project-root
  resolution instead of expanding `codex-config`;
- implement `ProjectAgentStore::bootstrap/load` over `ExecutorFileSystem`;
- add unit tests using an in-memory or test filesystem;
- add Cargo/Bazel workspace wiring.

Observable result: one shared, remotely usable API can resolve a project root and idempotently create
or load a valid empty `AGENT/registry.toml`.

### Slice 1B - Automatic project bootstrap

- add the initial `codex-project-agents-extension` crate;
- resolve the selected executor filesystem during thread startup;
- bootstrap/load the project catalog in a thread-lifecycle contributor;
- install the extension in app-server thread extensions;
- keep failures non-destructive and surface bounded startup diagnostics;
- add local and remote executor integration tests.

Observable result: opening a project through interactive `codex` or `codex exec` automatically creates
the registry once and never overwrites an existing registry.

### Slice 1C - CLI management

- add `agent_cmd.rs` and minimal `main.rs` wiring;
- implement list/show/create/disable with text and JSON output;
- add CLI integration tests.

Observable result: users and the main AGENT can manage stable project AGENT definitions without
manually editing registry structure.

### Stage 2 - Runtime delegation and isolation

- extend `codex-project-agents-extension`;
- expose enabled AGENT definitions as tools;
- add generic tool visibility policy;
- spawn constrained worker threads with final JSON schema;
- implement `.x` native/command/MCP executors;
- persist task summaries and candidates;
- add app-server/core integration tests.

### Stage 3 - Maintenance and TUI

- add manual maintenance engine and locks;
- add bounded accepted-memory retrieval;
- add typed maintenance-status notification;
- add TUI reminder/action and snapshots;
- add remote executor integration coverage.

## Validation routes

Each slice runs formatting automatically after code changes. Focused tests follow ownership:

```text
cd codex-rs && just test -p codex-project-agents
cd codex-rs && just test -p codex-cli
cd codex-rs && just test -p codex-project-agents-extension
cd codex-rs && just test -p codex-app-server
cd codex-rs && just test -p codex-core
cd codex-rs && just test -p codex-tui
```

Changes to shared config or protocol additionally require their schema generators and focused tests.
The complete test suite requires user approval after focused tests pass, per repository instructions.
This project uses GitHub Actions as the authoritative compilation environment. Local work records
formatting and non-compiling checks only; Rust compilation, focused tests, and lint results remain
pending until the workflow runs.
