<!-- CTF|认证 -->

# T-003 Project AGENT System Task Sheet

## Status

- State: active
- Current phase: capability inventory
- Objective owner: user-confirmed task sheet from this thread
- Durable task state: `task/current.yaml`
- Navigation source: `AGENTS_NAVIGATION.md`

## Final objective

Implement a project-local, file-defined, specialist AGENT system as a shared Codex capability. A
project AGENT is an intelligent tool controlled by the main AGENT, not an independent general-purpose
worker. Deliver the system through the interactive `codex` TUI, `codex exec`, and `codex agents`
management commands while reusing existing repository capabilities before adding new ones.

## Fixed product rules

1. Project discovery idempotently creates an `AGENT/` registry that the main AGENT can populate.
2. AGENT definitions are stable, long-lived, and scoped to one bounded unit of work.
3. An AGENT starts with no tools and sees only explicitly registered native tools or `.x` manifests.
4. A `.x` file is declarative metadata and references an external implementation; it never embeds a
   script body.
5. A child AGENT cannot change its definition, constraints, tool registry, or accepted long-term
   memory directly.
6. Out-of-scope work and missing capabilities return to the main AGENT through a fixed protocol.
7. Per-AGENT model configuration references existing Codex model/provider configuration by name.
8. Task summaries are recorded automatically. Memory and improvement suggestions remain candidates
   until a manual maintenance run accepts them.
9. Maintenance never interrupts active project work. The TUI may show a non-blocking reminder; the
   user starts maintenance manually.
10. Model-visible memory is retrieved selectively and always has a hard size cap.

## Required result protocol

```yaml
status: completed | rejected_out_of_scope | blocked_missing_tool | failed
agent_id:
task_id:
result:
artifacts: []
evidence: []
memory_candidates: []
improvement_proposals: []
error:
```

## Scope exclusions

- No unrestricted general-purpose child AGENT.
- No child-controlled cross-domain delegation.
- No child self-modification.
- No executable script bodies in `.x` manifests.
- No automatic maintenance while normal tasks are active.
- No unbounded context or full-memory injection.
- No provider URLs, credentials, or secrets stored in project AGENT definitions.
- No duplicate implementation where an existing Codex abstraction is sufficient.

## Capability reuse inventory

Fill this table before selecting implementation owners.

| Capability | Existing owner/path | Reuse as-is | Extend | Missing | Evidence/notes |
| --- | --- | --- | --- | --- | --- |
| Multi-agent orchestration | `core/src/agent/`, `core/src/agent/control/spawn.rs`, `core/src/tools/handlers/multi_agents_v2/` | Thread spawning, status, role/model override, agent graph | Project-defined specialist resolution and fixed result schema | No project registry or agent-as-tool catalog | Existing roles are config-backed generic spawn roles with full thread behavior |
| Tool registry and dispatch | `tools/`, `ext/extension-api`, `core/src/tools/{registry,router,spec_plan}.rs` | `ToolExecutor`, `ToolContributor`, `ExtensionToolAdapter`, dispatch lifecycle | Project `.x` executors and post-plan visibility policy | No project-local manifest loader | Extension contributors are already the preferred route outside `codex-core` |
| Native tool filtering | `core/src/tools/spec_plan.rs`; MCP-only filter in `codex-mcp/src/tools.rs` | MCP `enabled_tools`/`disabled_tools` semantics | Generic model-visible and dispatch-time allowlist | Core-wide native-tool allowlist is absent | Filtering must cover both advertised specs and runtime dispatch |
| Skills/plugins/apps | `ext/skills`, `core-skills`, `plugin`, `core/src/plugins/` | Catalogs, providers, context/tool contributors | Per-agent explicit registration references | No specialist ownership boundary | Do not build another skill/plugin discovery system |
| MCP tool mutation | `codex-mcp/src/connection_manager.rs`, `codex-mcp/src/tools.rs` | Server lifecycle and tool allow/deny filtering | Resolve `.x` references to existing MCP tools where declared | No project-agent mapping | Existing connection manager remains the mutation owner |
| Model/provider selection | `core/src/config/agent_roles.rs`, `core/src/agent/role.rs` | Full role config layer, model, reasoning, named provider | Load project AGENT definitions into the existing role/config path | No project registry source | Role config already preserves caller provider unless explicitly overridden |
| Project/config discovery | `config/src/loader/mod.rs`, `config/src/project_root_markers.rs` | Marker semantics and `ExecutorFileSystem` support | Expose/reuse project-root resolution for AGENT storage | Public project-root resolver is absent | Private `find_project_root` already supports remote executor filesystems |
| Thread/task persistence | `thread-store`, `rollout`, `state`, `agent-graph-store` | Thread history, status, graph, SQLite metadata | Link project task summaries to spawned thread ids | File-backed AGENT registry/history schema | Runtime sessions should keep using existing thread persistence |
| Memory retrieval/context | `memories/read`, `memories/write`, `ext/memories` | Two-phase extraction/consolidation patterns, extension context/tools | Per-agent candidate queues, manual promotion, bounded retrieval | No project-local per-agent memory store | Existing memory is global and startup-driven, not manual per specialist |
| CLI command dispatch | `cli/src/main.rs`, `cli/src/mcp_cmd.rs`, `cli/src/plugin_cmd.rs` | Clap dispatch and JSON/text output conventions | Add a dedicated small `agent_cmd.rs` module | No `codex agents` command | Keep changes out of the already-large `main.rs` except wiring |
| TUI notices/actions | `tui/src/lib.rs`, `tui/src/app/`, app-server event pipeline | Existing warning/history rendering and snapshot framework | Add pending-maintenance status and manual action | Extension event sink currently forwards only goal updates | Remote app-server/exec layouts rule out local-only filesystem checks |
| Integration-test support | `core/tests/suite`, `app-server/tests/suite/v2`, CLI tests, TUI snapshots | Existing builders, remote-executor helpers, snapshots | Add project-agent fixtures and end-to-end cases | No specialist-agent fixture | Agent logic requires integration coverage; UI changes require snapshots |

## Delivery stages

### Stage 0 - Existing capability inventory and architecture

- [x] Inspect only the documented navigation routes first.
- [x] Record existing implementations and current progress in the reuse inventory.
- [x] Identify the smallest crate/API ownership boundary without defaulting to `codex-core`.
- [x] Define on-disk schemas, Rust types, API boundaries, context limits, and migration behavior.
- [x] Split implementation into reviewable changes consistent with repository change-size rules.

### Stage 1 - Registry and CLI management

- [x] Idempotent project `AGENT/` bootstrap implementation.
- [x] Registry and AGENT definition schemas with validation and versioning.
- [x] Declarative `.x` tool manifest schema.
- [ ] `codex agents list/show/create/disable` management surface.
- [x] Cross-platform `PathUri` handling and focused test coverage authored.
- [ ] GitHub Actions compilation, focused tests, and lint validation.

### Stage 2 - Runtime delegation and isolation

- [ ] Expose each registered project AGENT to the main AGENT as a tool.
- [ ] Build bounded child context from definition, constraints, task, registered tools, and retrieved
      memory.
- [ ] Enforce domain and tool visibility boundaries.
- [ ] Parse and validate the fixed result protocol.
- [ ] Support configured model, reasoning effort, and named provider.
- [ ] Return out-of-scope or missing-tool work to the main AGENT without cross-domain execution.
- [ ] Add agent-logic integration tests.

### Stage 3 - History, memory candidates, maintenance, and TUI

- [ ] Persist bounded task summaries and proposal queues.
- [ ] Index and retrieve accepted long-term memory under hard context limits.
- [ ] Add manual maintenance processing with evidence, deduplication, decisions, and change history.
- [ ] Detect pending maintenance on project load/refresh.
- [ ] Show a non-blocking TUI reminder and manual action.
- [ ] Add TUI snapshot coverage and persistence integration tests.

## Acceptance checklist

- [ ] Reopening a project does not overwrite an existing registry.
- [ ] The CLI can discover, create, inspect, and disable project AGENT definitions.
- [ ] The main AGENT can invoke an appropriate registered AGENT as a tool.
- [ ] A child cannot see or call an unregistered tool.
- [ ] An out-of-scope task returns `rejected_out_of_scope` without executing it.
- [ ] A missing capability returns `blocked_missing_tool` without self-installing a tool.
- [ ] Normal work cannot silently rewrite definitions, constraints, tools, or accepted memory.
- [ ] Pending proposals cause a non-blocking TUI reminder only.
- [ ] Manual maintenance produces an attributable decision and reloads accepted changes.
- [ ] Every model-visible fragment is bounded and no injected item exceeds repository context limits.
- [ ] Linux, macOS, and Windows paths and behavior are covered where applicable.
- [ ] `AGENTS_NAVIGATION.md` is updated with final ownership and focused validation routes.

## Decision log

| Date | Decision | Evidence/reason | Consequence |
| --- | --- | --- | --- |
| 2026-08-10 | Shared core capability with CLI/TUI/exec surfaces | User confirmation | Avoid separate orchestration engines |
| 2026-08-10 | `.x` is manifest-only | User confirmation | Implementations remain independently editable |
| 2026-08-10 | Manual maintenance with TUI reminder | User confirmation | No automatic interruption of active work |
| 2026-08-10 | Inventory existing tools before design | User confirmation | Reuse map is a blocking Stage 0 deliverable |
| 2026-08-10 | Build on extension contributors and existing agent-role spawning | Repository inspection | Keep the new feature outside `codex-core` except generic host gaps |
| 2026-08-10 | Reuse `codex-file-system` PathUri find-up API | `find_nearest_ancestor_with_markers` already provides the required remote-safe primitive | Do not expose or duplicate the private `codex-config` native-path helper |
| 2026-08-10 | GitHub Actions is the authoritative Rust build environment | User clarification; the local machine has no supported linker toolchain | Do not provision a local compiler; track compilation/tests/lints as workflow validation |

## Open issues and blockers

None at task start. Add only issues that can change implementation or acceptance.

## Work log

| Work unit | Result | Validation | Next action |
| --- | --- | --- | --- |
| Task initialization | Requirements confirmed; durable sheet created | User confirmation | Inventory documented repository routes |
| Capability inventory pass 1 | Existing agent roles, thread spawning, extension tools, MCP filters, memories, config discovery, CLI and TUI routes mapped | Exact paths and representative symbols inspected | Resolve architecture ownership and generic tool-isolation gap |
| Architecture | New foundation and extension crates selected; existing roles, extension tools, worker threads, remote filesystem and memories will be reused | `T-003-project-agent-architecture.md` | Implement Stage 1 Slice 1A foundation |
| Slice 1A implementation draft | Added `codex-project-agents`, Cargo/Bazel wiring, registry/definition/tool/result models, path validation, project-root resolution, idempotent store, and focused tests | `just fmt` passed; compilation was not run locally by project policy | Update lockfiles/static review, then validate with GitHub Actions |
