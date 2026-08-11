<!-- CTF|认证 -->

# Codex Repository Navigation

This is the agent-facing repository map for locating implementation code without repeating a broad
repository scan. Read the root `AGENTS.md` first; this file supplements its rules and does not
replace them.

## Navigation contract

1. Start with the feature route in this file, then inspect only the listed source-of-truth path and
   the next hop required by the change.
2. Treat paths and symbols recorded here as verified navigation anchors, not as a substitute for
   reading the implementation that will be changed.
3. Do not rescan the whole repository when an anchor still exists and the observed call relation
   matches it.
4. Revalidate the smallest affected area only when at least one invalidation condition is present:
   - a recorded file or symbol no longer exists;
   - imports or crate dependencies contradict the recorded route;
   - compilation or tests expose a different owner;
   - observed runtime behavior contradicts the recorded flow;
   - the requested feature is not represented here.
5. When a change adds, moves, or removes a feature entrypoint, public protocol, core owner,
   persistence boundary, or primary test location, update this file in the same change.
6. Keep this file selective. Record stable entrypoints and boundaries; do not copy the full tree or
   list every helper module.

## Instruction scope

- `AGENTS.md` at the repository root applies repository-wide.
- `codex-rs/tui/src/bottom_pane/AGENTS.md` adds local rules for the bottom-pane state machines.
  Read it before changing `chat_composer.rs`, `paste_burst.rs`, or related behavior in that folder.
- If another nested `AGENTS.md` is introduced, its instructions take precedence for files in its
  subtree and this section must be updated.

## Main runtime flow

```text
codex-rs/cli (`codex` multitool)
  -> interactive: codex-rs/tui
  -> non-interactive: codex-rs/exec
  -> service/API: codex-rs/app-server

tui or exec
  -> codex-rs/app-server-client
  -> embedded, local, or remote app-server
  -> codex-rs/app-server-protocol (JSON-RPC Thread / Turn / Item API)
  -> codex-rs/core (thread, session, model, context, tool orchestration)
  -> model provider and/or tool execution
  -> codex-rs/exec-server, local sandbox, MCP, or extensions
  -> codex-rs/thread-store + codex-rs/rollout + codex-rs/state
  -> streamed events return through app-server-client to the surface
```

Important variations:

- The TypeScript SDK in `sdk/typescript` launches the Codex CLI and consumes JSONL events.
- The Python SDK in `sdk/python` launches `codex app-server --listen stdio://` and uses typed
  JSON-RPC.
- `codex-rs/mcp-server` exposes Codex itself as an MCP server. This is separate from
  `codex-rs/codex-mcp`, which manages MCP servers and tools consumed by Codex.
- `codex-rs/exec-server` is the process/filesystem execution service, not the non-interactive
  `codex exec` surface in `codex-rs/exec`.

## Top-level repository map

| Path                                    | Ownership                                             | Start here when changing                                                      |
| --------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------- |
| `codex-rs/`                             | Main Rust workspace and product implementation        | Agent runtime, CLI/TUI, app-server, protocols, tools, sandboxing, persistence |
| `codex-cli/`                            | npm package that dispatches the platform Codex binary | npm CLI packaging, binary selection, release wrapper behavior                 |
| `sdk/typescript/`                       | TypeScript SDK over CLI JSONL                         | `Codex`, `Thread`, streamed events, TS packaging                              |
| `sdk/python/`                           | Python SDK over app-server JSON-RPC                   | Python public API, typed RPC, lifecycle, login, approvals                     |
| `sdk/python-runtime/`                   | Platform runtime wheel used by the Python SDK         | Bundled binary discovery and Python release packaging                         |
| `docs/`                                 | Repository contributor/install/config documentation   | Repository documentation only; do not add general product docs here           |
| `scripts/`                              | Formatting, release, build, and maintenance scripts   | Repository automation                                                         |
| `tools/`                                | Repository development tools                          | Lints and maintenance tooling                                                 |
| `bazel/`, `BUILD.bazel`, `MODULE.bazel` | Bazel build graph                                     | Bazel targets, runfiles, dependency lock integration                          |
| `.github/`                              | CI and GitHub automation                              | Workflows, issue/PR automation, release jobs                                  |
| `justfile`                              | Primary local Rust workflow recipes                   | Formatting, tests, schema generation, Clippy fixes                            |

## Product surfaces and API routes

### `codex` command dispatch

- Start: `codex-rs/cli/src/main.rs`
- Representative symbols: `MultitoolCli`, `Subcommand`, `main`
- Follow to:
  - project-local specialist AGENT management: `codex-rs/cli/src/agent_cmd.rs`;
  - interactive default and session pickers: `codex-rs/tui/src/lib.rs`;
  - `codex exec` and `codex review`: `codex-rs/exec/src/lib.rs`;
  - `codex app-server`: `codex-rs/app-server/src/lib.rs`;
  - `codex mcp-server`: `codex-rs/mcp-server/src/lib.rs`;
  - `codex exec-server`: `codex-rs/exec-server/src/server.rs`.
- Focused validation: `cd codex-rs && just test -p codex-cli`

### Interactive TUI

- Bootstrap: `codex-rs/tui/src/lib.rs`
- Representative symbols: `run_main`, `run_ratatui_app`, `App::run`
- Main orchestration and event loop: `codex-rs/tui/src/app.rs`, `codex-rs/tui/src/app/`
- Conversation UI: `codex-rs/tui/src/chatwidget.rs`, `codex-rs/tui/src/chatwidget/`
- Slash-command registry and dispatch: `codex-rs/tui/src/slash_command.rs`,
  `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
- Project AGENT maintenance UI: `codex-rs/tui/src/app/project_agent_maintenance.rs`,
  `codex-rs/tui/src/chatwidget/project_agent_maintenance.rs`
- Project AGENT workbench UI: `codex-rs/tui/src/app/project_agent_workbench.rs`,
  `codex-rs/tui/src/chatwidget/project_agent_workbench.rs`
- Composer and approval UI: `codex-rs/tui/src/bottom_pane/`
- Rendered history/tool cells: `codex-rs/tui/src/history_cell/`,
  `codex-rs/tui/src/exec_cell/`, `codex-rs/tui/src/diff_render.rs`
- Resume/session flows: `codex-rs/tui/src/resume_picker.rs`,
  `codex-rs/tui/src/session_resume.rs`, `codex-rs/tui/src/session_archive_commands.rs`
- Snapshot location: `codex-rs/tui/src/snapshots/`
- Focused validation:
  - `cd codex-rs && just test -p codex-tui`
  - `cd codex-rs && cargo insta pending-snapshots -p codex-tui`
  - intentionally accept all pending TUI snapshots only with
    `cargo insta accept -p codex-tui`

### Non-interactive execution and review

- Start: `codex-rs/exec/src/lib.rs`
- Representative symbols: `run_main`, `run_exec_session`
- CLI arguments: `codex-rs/exec/src/cli.rs`
- Event rendering: `codex-rs/exec/src/event_processor.rs`,
  `codex-rs/exec/src/event_processor_with_jsonl_output.rs`
- App-server bridge: `codex-rs/app-server-client/src/lib.rs`
- Focused validation: `cd codex-rs && just test -p codex-exec`

### App-server client boundary

- Start: `codex-rs/app-server-client/src/lib.rs`
- Representative symbols: `InProcessAppServerClient`, `AppServerClient`
- This is the shared client facade for CLI surfaces and supports in-process and remote app-server
  connections.
- Change this layer when transport/client lifecycle behavior must be shared by TUI and exec without
  duplicating app-server plumbing.
- Focused validation: `cd codex-rs && just test -p codex-app-server-client`

### App-server implementation

- Runtime/bootstrap: `codex-rs/app-server/src/lib.rs`
- Representative symbols: `run_main`, `run_main_with_transport_options`
- Request routing: `codex-rs/app-server/src/message_processor.rs` (`MessageProcessor`)
- Request handlers: `codex-rs/app-server/src/request_processors.rs`
- Project AGENT roster/detail and maintenance handler:
  `codex-rs/app-server/src/request_processors/project_agent_processor.rs`
- Event translation: `codex-rs/app-server/src/bespoke_event_handling.rs`
- Outbound delivery: `codex-rs/app-server/src/outgoing_message.rs`
- Thread status/state: `codex-rs/app-server/src/thread_state.rs`,
  `codex-rs/app-server/src/thread_status.rs`
- Public behavior reference: `codex-rs/app-server/README.md`
- Integration tests: `codex-rs/app-server/tests/suite/v2/`
- Focused validation: `cd codex-rs && just test -p codex-app-server`

### App-server protocol v2

- RPC envelope: `codex-rs/app-server-protocol/src/rpc.rs`
- Request/notification registry and experimental gating:
  `codex-rs/app-server-protocol/src/protocol/common.rs`
- Active v2 surface: `codex-rs/app-server-protocol/src/protocol/v2/`
- Thread API: `codex-rs/app-server-protocol/src/protocol/v2/thread.rs`
  - representative types: `ThreadStartParams`, `ThreadStartResponse`, `ThreadResumeParams`,
    `ThreadListParams`;
- Turn API: `codex-rs/app-server-protocol/src/protocol/v2/turn.rs`
  - representative types: `TurnStartParams`, `TurnStartResponse`, `TurnSteerParams`,
    `TurnInterruptParams`;
- Project AGENT management API: `codex-rs/app-server-protocol/src/protocol/v2/project_agent.rs`
  - representative types: `ThreadProjectAgentListParams`, `ThreadProjectAgentReadParams`,
    `ThreadProjectAgentFollowUpParams`, `ThreadProjectAgentTerminateParams`,
    `ThreadProjectAgentRetryParams`, `ThreadProjectAgentRebuildParams`,
    `ThreadProjectAgentReadResponse`, `ThreadProjectAgentMaintenanceRunParams`,
    `ThreadProjectAgentMaintenanceRunResponse`,
    `ThreadProjectAgentMaintenanceStatusUpdatedNotification`;
- Other API areas are split into matching files such as `command_exec.rs`, `fs.rs`, `mcp.rs`,
  `plugin.rs`, `permissions.rs`, `realtime.rs`, and `remote_control.rs`.
- When the wire shape changes, also update `codex-rs/app-server/README.md` and generated schema
  fixtures.
- Focused validation:
  - `cd codex-rs && just write-app-server-schema`
  - use `just write-app-server-schema --experimental` when experimental fixtures change;
  - `cd codex-rs && just test -p codex-app-server-protocol`.

## Agent runtime routes

### Thread, session, and turn lifecycle

- Thread ownership: `codex-rs/core/src/thread_manager.rs`
  - representative symbols: `ThreadManager`, `NewThread`;
- Loaded thread facade: `codex-rs/core/src/codex_thread.rs`
  - representative symbol: `CodexThread`;
- Session state: `codex-rs/core/src/session/session.rs`
  - representative symbol: `Session`;
- Turn execution: `codex-rs/core/src/session/turn.rs`
- Per-turn settings/context: `codex-rs/core/src/session/turn_context.rs`
  - representative symbol: `TurnContext`;
- Session event handlers and input queue: `codex-rs/core/src/session/handlers.rs`,
  `codex-rs/core/src/session/input_queue.rs`
- Prefer integration tests under `codex-rs/core/tests/suite/` for agent behavior.
- Focused validation: `cd codex-rs && just test -p codex-core`
- Per root policy, ask before running the complete workspace `just test` after common/core/protocol
  changes.

### Model requests, streaming, and providers

- Responses/model client: `codex-rs/core/src/client.rs`
  - representative symbols: `ModelClient`, `ModelClientSession`;
- Prompt and response types: `codex-rs/core/src/client_common.rs`
- Provider registry: `codex-rs/model-provider-info/src/lib.rs`
  - representative symbols: `ModelProviderInfo`, `WireApi`, `create_openai_provider`,
    `create_amazon_bedrock_provider`, `create_oss_provider`;
- Provider-specific auth/runtime: `codex-rs/model-provider/`
- ChatGPT/backend calls: `codex-rs/backend-client/`, `codex-rs/codex-api/`,
  `codex-rs/codex-client/`
- Authentication: `codex-rs/login/src/lib.rs` (`AuthManager`, login/logout functions)
- Local OSS providers: `codex-rs/ollama/`, `codex-rs/lmstudio/`
- Model catalog and selection UI: `codex-rs/models-manager/`,
  `codex-rs/tui/src/model_catalog.rs`
- Start tests in the crate that owns the change; use core integration tests for end-to-end model
  request behavior.

### Context, prompts, instructions, and configuration

- Context history owner: `codex-rs/core/src/context_manager/history.rs`
  - representative symbol: `ContextManager`;
- Model-visible fragments: `codex-rs/core/src/context/`
- AGENTS instruction loading: `codex-rs/core/src/agents_md.rs`,
  `codex-rs/core/src/agents_md_manager.rs`
- Config loading and layer precedence: `codex-rs/config/src/loader/`
- Effective core config types: `codex-rs/core/src/config/`
- Feature registry: `codex-rs/features/src/lib.rs`
  - representative symbols: `Feature`, `Features`, `FEATURES`;
- Shared prompt assets: `codex-rs/prompts/`,
  `codex-rs/collaboration-mode-templates/`
- Context fragment contracts: `codex-rs/context-fragments/`
- If `ConfigToml` or nested types change, run:
  - `cd codex-rs && just write-config-schema`
  - `cd codex-rs && just test -p codex-core`.
- Agent-logic changes require an integration test in `codex-rs/core/tests/suite/`.

### Review, guardian, multi-agent, and realtime behavior

- Review flow: `codex-rs/core/src/session/review.rs`, `codex-rs/prompts/`
- Guardian: `codex-rs/core/src/guardian/`, `codex-rs/ext/guardian/`
- Agent delegation: `codex-rs/core/src/agent/`, `codex-rs/core/src/codex_delegate.rs`,
  `codex-rs/core/src/session/multi_agents.rs`
- Multi-agent tool handlers: `codex-rs/core/src/tools/handlers/multi_agents*.rs`
- Realtime conversation: `codex-rs/core/src/realtime_conversation.rs`,
  `codex-rs/realtime-webrtc/`, `codex-rs/app-server-protocol/src/protocol/v2/realtime.rs`
- Representative integration tests:
  `codex-rs/core/tests/suite/auto_review.rs`,
  `codex-rs/core/tests/suite/guardian_review.rs`,
  `codex-rs/core/tests/suite/multi_agent_mode.rs`,
  `codex-rs/core/tests/suite/realtime_conversation.rs`.

### Project-local specialist AGENT definitions

- Foundation crate: `codex-rs/project-agents/`
  - representative symbols: `ProjectAgentRegistry`, `ProjectAgentDefinition`,
    `ProjectAgentToolManifest`, `ProjectAgentCommandToolRegistration`,
    `ProjectAgentTaskResult`, `ProjectAgentTaskMetadata`,
    `ProjectAgentSessionMetadata`, `ProjectAgentStore`, `ProjectAgentEntry`,
    `ProjectAgentMaintenanceOutcome`;
  - task/session persistence and bounded history reads:
    `codex-rs/project-agents/src/task_store.rs`;
  - maintenance owner: `codex-rs/project-agents/src/maintenance_store.rs`
    (`ProjectAgentStore::maintenance_status`, `ProjectAgentStore::maintain`);
- CLI management: `codex-rs/cli/src/agent_cmd.rs`
  - representative symbols: `AgentCli`, `AgentSubcommand`;
  - commands: `codex agents list`, `show`, `create`, `add-command-tool`, `disable`, and
    `maintain`;
- Runtime extension: `codex-rs/ext/project-agents/`
  - representative symbols: `ProjectAgentExtension`, `ProjectAgentRootContext`,
    `ProjectAgentWorkerContext`, `list_thread_project_agents`, `read_thread_project_agent`,
    `follow_up_thread_project_agent`, `terminate_thread_project_agent`,
    `retry_thread_project_agent`, `rebuild_thread_project_agent_session`,
    `maintain_thread_project_agents`;
  - bounded roster/detail inspection owner: `codex-rs/ext/project-agents/src/inspection.rs`;
  - follow-up, termination, retry, and explicit rebuild controls:
    `codex-rs/ext/project-agents/src/control.rs`;
  - reusable worker-thread lifecycle, per-AGENT task serialization, and task/session phase
    persistence: `codex-rs/ext/project-agents/src/worker.rs`;
- App-server surface: `thread/projectAgent/list`, `thread/projectAgent/read`,
  `thread/projectAgent/followUp`, `thread/projectAgent/terminate`,
  `thread/projectAgent/retry`, `thread/projectAgent/rebuild`,
  `thread/projectAgentMaintenance/run`, and `thread/projectAgentMaintenance/statusUpdated`;
- TUI surfaces: `/agents` opens a searchable `@agent-name` roster/detail/control workbench;
  `/agents-maintain` applies pending maintenance, with reminders deferred while a turn is active.
- Cross-executor project-root discovery reuses
  `codex_file_system::find_nearest_ancestor_with_markers` with `PathUri`.
- On-disk source of truth begins at `AGENT/registry.toml`; discovery never activates an unregistered
  directory implicitly.
- Focused tests: `codex-rs/project-agents/src/tests.rs`, `codex-rs/cli/tests/agents.rs`,
  `codex-rs/ext/project-agents/src/tests.rs`, and
  `codex-rs/app-server/tests/suite/v2/project_agents.rs`,
  `codex-rs/app-server/tests/suite/v2/project_agent_controls.rs`, plus
  `codex-rs/tui/src/chatwidget/tests/project_agent_maintenance_tests.rs` and
  `codex-rs/tui/src/chatwidget/tests/project_agent_workbench_tests.rs`.
- Authoritative validation runs in GitHub Actions:
  `cd codex-rs && just test -p codex-project-agents`, `just test -p codex-cli`,
  `just test -p codex-project-agents-extension`, `just test -p codex-app-server-protocol`,
  `just test -p codex-app-server`, and `just test -p codex-tui`.

## Tools and extension routes

### Tool planning, registry, and dispatch

- Shared host tool contracts: `codex-rs/tools/src/lib.rs`
  - representative symbols: `ToolSpec`, `ToolCall`, `ToolExecutor`, `ToolOutput`;
- Tool planning: `codex-rs/core/src/tools/spec_plan.rs`
  - representative function: `build_tool_router`;
- Router: `codex-rs/core/src/tools/router.rs` (`ToolRouter`)
- Registry: `codex-rs/core/src/tools/registry.rs` (`ToolRegistry`)
- Dispatch/orchestration: `codex-rs/core/src/tools/orchestrator.rs`,
  `codex-rs/core/src/tools/lifecycle.rs`
- Built-in handlers: `codex-rs/core/src/tools/handlers/`
  - shell/exec: `shell.rs`, `unified_exec.rs`;
  - patches: `apply_patch.rs`;
  - MCP: `mcp.rs`, `mcp_resource.rs`;
  - plan/input/permissions: `plan.rs`, `request_user_input.rs`, `request_permissions.rs`;
  - images and discovery: `view_image.rs`, `tool_search.rs`;
  - multi-agent and jobs: `multi_agents*.rs`, `agent_jobs.rs`.
- Reusable extension API: `codex-rs/ext/extension-api/src/lib.rs`
- Prefer extracting reusable host machinery to `codex-tools` or an appropriate extension crate
  rather than growing `codex-core`.
- Focused validation:
  - `cd codex-rs && just test -p codex-tools`
  - `cd codex-rs && just test -p codex-core` for core routing behavior.

### MCP

- Connection lifecycle and tool-call mutation:
  `codex-rs/codex-mcp/src/connection_manager.rs` (`McpConnectionManager`)
- Server catalog/conflicts: `codex-rs/codex-mcp/src/catalog.rs`
- Low-level RMCP client: `codex-rs/codex-mcp/src/rmcp_client.rs`
- Resource client: `codex-rs/codex-mcp/src/resource_client.rs`
- Core session integration: `codex-rs/core/src/mcp.rs`,
  `codex-rs/core/src/session/mcp.rs`, `codex-rs/core/src/mcp_tool_call.rs`
- Model-visible handlers: `codex-rs/core/src/tools/handlers/mcp.rs`,
  `codex-rs/core/src/tools/handlers/mcp_resource.rs`
- App-server MCP API: `codex-rs/app-server-protocol/src/protocol/v2/mcp.rs`
- Representative integration tests live under `codex-rs/core/tests/suite/mcp_*.rs` and
  `codex-rs/app-server/tests/suite/v2/mcp_*.rs`.
- Focused validation: `cd codex-rs && just test -p codex-mcp`

### Skills, plugins, apps, and connectors

- Embedded/system skills: `codex-rs/skills/`
- Core skill loading/injection: `codex-rs/core/src/skills.rs`, `codex-rs/core-skills/`
- Shared plugin models: `codex-rs/plugin/`
- Core plugin assembly: `codex-rs/core/src/plugins/`, `codex-rs/core-plugins/`
- Apps and connector runtime: `codex-rs/core/src/apps/`, `codex-rs/connectors/`,
  `codex-rs/ext/connectors/`
- Extension implementations: `codex-rs/ext/`
- App-server API files: `codex-rs/app-server-protocol/src/protocol/v2/plugin.rs`,
  `apps.rs`, `hook.rs`, and related request handlers in `codex-rs/app-server/`
- Representative tests:
  `codex-rs/core/tests/suite/plugins.rs`,
  `codex-rs/core/tests/suite/hooks.rs`,
  `codex-rs/app-server/tests/suite/v2/plugin_*.rs`,
  `codex-rs/app-server/tests/suite/v2/skills_list.rs`.

### Code mode

- Protocol: `codex-rs/code-mode-protocol/`
- In-process code-mode runtime: `codex-rs/code-mode/`
- Host connection and delegation: `codex-rs/code-mode-host/src/lib.rs`
- Core integration: `codex-rs/core/src/tools/code_mode/`
- Integration tests: `codex-rs/core/tests/suite/code_mode.rs`,
  `codex-rs/core/tests/suite/code_mode_elicitation.rs`

## Execution, permissions, and sandbox routes

### Process and filesystem execution

- Execution service exports: `codex-rs/exec-server/src/lib.rs`
  - representative symbols: `ExecServerClient`, `EnvironmentManager`, `ExecProcess`, `run_main`;
- Server transport: `codex-rs/exec-server/src/server.rs`
- Process implementations: `codex-rs/exec-server/src/local_process.rs`,
  `remote_process.rs`, `process.rs`
- Filesystem implementations: `codex-rs/exec-server/src/local_file_system.rs`,
  `remote_file_system.rs`, `sandboxed_file_system.rs`
- Remote relay/environment: `codex-rs/exec-server/src/remote.rs`,
  `environment.rs`, `noise_relay.rs`
- Core long-lived exec state: `codex-rs/core/src/unified_exec/mod.rs`
  - representative symbols: `UnifiedExecContext`, `UnifiedExecProcessManager`;
- Tool runtime bridge: `codex-rs/core/src/tools/runtimes/unified_exec.rs`
- Focused validation: `cd codex-rs && just test -p codex-exec-server`

### Sandbox, command policy, and approvals

- Cross-platform policy transforms: `codex-rs/sandboxing/`
- Linux launcher and Bubblewrap/Landlock behavior: `codex-rs/linux-sandbox/`
- Windows sandbox: `codex-rs/windows-sandbox-rs/`,
  `codex-rs/core/src/windows_sandbox.rs`
- Bubblewrap helper: `codex-rs/bwrap/`
- Command policy language: `codex-rs/execpolicy/`
  - representative symbols: `Policy`, `Evaluation`, `Decision`, `PolicyParser`;
- Shared command parsing/safety: `codex-rs/shell-command/`
- Shell escalation helpers: `codex-rs/shell-escalation/`
- Network proxy/policy: `codex-rs/network-proxy/`,
  `codex-rs/core/src/network_policy_decision.rs`
- Core approvals: `codex-rs/core/src/tools/approvals.rs`,
  `codex-rs/core/src/tools/network_approval.rs`
- Focused validation:
  - `cd codex-rs && just test -p codex-execpolicy`
  - use the owning sandbox crate test plus `just test -p codex-core` when core approval behavior
    changes.

## Persistence and memory routes

### Threads, rollouts, and SQLite state

- Storage-neutral boundary: `codex-rs/thread-store/src/lib.rs`
  - representative symbols: `ThreadStore`, `LiveThread`, `LocalThreadStore`;
- JSONL rollout storage/discovery: `codex-rs/rollout/src/lib.rs`
  - representative symbol: `RolloutRecorder`;
- SQLite-backed metadata/state: `codex-rs/state/src/lib.rs`
  - representative symbol: `StateRuntime`;
- Core bridge/bootstrap: `codex-rs/core/src/state_db_bridge.rs`,
  `codex-rs/core/src/rollout.rs`, `codex-rs/core/src/thread_manager.rs`
- Global prompt history file: `codex-rs/message-history/src/lib.rs`
- Spawned-agent topology: `codex-rs/agent-graph-store/src/lib.rs`
  - representative symbol: `AgentGraphStore`;
- App-server thread API tests: `codex-rs/app-server/tests/suite/v2/thread_*.rs`
- Focused validation:
  - `cd codex-rs && just test -p codex-thread-store`
  - `cd codex-rs && just test -p codex-rollout`
  - `cd codex-rs && just test -p codex-state`.

### Memories

- Read path: `codex-rs/memories/read/src/lib.rs`
- Write/startup pipeline: `codex-rs/memories/write/src/lib.rs`
  - representative function: `start_memories_startup_task`;
- Phase implementations: `codex-rs/memories/write/src/phase1.rs`, `phase2.rs`
- Memory prompts: `codex-rs/memories/write/templates/memories/`
- Memory extension/tools: `codex-rs/ext/memories/`
- Database records: `codex-rs/state/`
- Focused validation:
  - `cd codex-rs && just test -p codex-memories-read`
  - `cd codex-rs && just test -p codex-memories-write`.

## SDK routes

### TypeScript SDK

- Public entry: `sdk/typescript/src/index.ts`
- Client: `sdk/typescript/src/codex.ts` (`Codex`)
- Conversation API: `sdk/typescript/src/thread.ts` (`Thread`, `run`, `runStreamed`)
- CLI process/JSONL transport: `sdk/typescript/src/exec.ts`
- Event and item contracts: `sdk/typescript/src/events.ts`, `items.ts`
- Tests: `sdk/typescript/tests/`
- Focused validation:
  - `cd sdk/typescript && pnpm test`
  - `cd sdk/typescript && pnpm lint`
  - `cd sdk/typescript && pnpm format`.

### Python SDK

- Public exports: `sdk/python/src/openai_codex/__init__.py`
- High-level API: `sdk/python/src/openai_codex/api.py` (`Codex`, `AsyncCodex`, thread/turn types)
- JSON-RPC client and process lifecycle: `sdk/python/src/openai_codex/client.py`
  - representative symbol: `CodexClient`;
- Generated v2 contracts: `sdk/python/src/openai_codex/generated/`
- Tests: `sdk/python/tests/`
- Runtime wheel: `sdk/python-runtime/`
- Focused validation:
  - `cd sdk/python && uv run --group test pytest`
  - `cd sdk/python && uv run --group format ruff check .`.

## Cross-cutting infrastructure

- Feature and product analytics: `codex-rs/analytics/`
- OpenTelemetry: `codex-rs/otel/`, `codex-rs/core/src/otel_init.rs`
- Feedback: `codex-rs/feedback/`
- Rollout trace/replay: `codex-rs/rollout-trace/`
- Response diagnostics: `codex-rs/response-debug-context/`
- Shared filesystem/path/process helpers: `codex-rs/file-system/`, `codex-rs/git-utils/`,
  `codex-rs/utils/`
- Build-time file reads must also be represented in the owning crate's `BUILD.bazel` data fields.
- Rust dependency changes require `just bazel-lock-update` from the repository root, as described
  in `AGENTS.md`.

## Validation decision table

| Change                    | Minimum validation before final formatting/fix                                 |
| ------------------------- | ------------------------------------------------------------------------------ |
| Rust crate implementation | `cd codex-rs && just test -p <crate>`                                          |
| Agent logic               | Add/run an integration test under `codex-rs/core/tests/suite/`                 |
| TUI-visible output        | `just test -p codex-tui`, inspect pending snapshots, accept intended snapshots |
| App-server v2 API         | Update README/schema fixtures; test protocol and app-server crates             |
| `ConfigToml` shape        | `just write-config-schema` plus owning tests                                   |
| Rust dependency           | `just bazel-lock-update` plus owning tests                                     |
| TypeScript SDK            | `pnpm test`, `pnpm lint`, `pnpm format` in `sdk/typescript`                    |
| Python SDK                | pytest and Ruff through the pyproject dependency groups                        |
| Repository Markdown only  | Validate referenced anchors and run the repository Markdown formatter/checker  |

After code changes, follow the root `AGENTS.md` ordering: run relevant tests first, then the scoped
`just fix -p <project>` when required, and finish with `cd codex-rs && just fmt`. Do not rerun tests
after `fix` or `fmt`.

## Maintenance checklist for this file

Update this navigation in the same change when any answer is yes:

- Did a user-facing command or surface move to another crate?
- Did TUI/exec stop using the current app-server-client boundary?
- Did an app-server v2 method move between protocol modules?
- Did thread/session/tool/model ownership move out of or into `codex-core`?
- Did a tool runtime, MCP manager, sandbox, or exec-server boundary change?
- Did thread persistence, rollout, SQLite, memory, or agent graph ownership change?
- Did an SDK switch transport or public entrypoint?
- Did a primary integration-test or snapshot location change?
- Was a nested `AGENTS.md` added, moved, or removed?

When updating an entry, verify the path, the representative symbol, the next-hop relationship, and
the focused validation command. Remove obsolete anchors rather than retaining historical routes.
