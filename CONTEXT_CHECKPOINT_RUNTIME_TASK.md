# Controlled Context Checkpoint Runtime

## Status

Source ownership and integration points were verified against the current tree on 2026-08-15.
Implementation is now the active follow-up to the Project AGENT return-path build. This document is
the fixed task definition and source map; implementation must not silently narrow or replace its
objective.

### Implementation checkpoint

- `d4ac1eee0` established the default-on runtime, typed IDs, pressure policy, Artifact capture,
  ToolGroup settlement, TurnRecord projection, model tools, pending installation, rollout metadata,
  and legacy fallback routing.
- The current recovery stage archives pre-truncation shell bytes and raw MCP JSON, uses typed
  contextual fragments, writes content-addressed manifest snapshots, retains a durable Artifact
  index, and reconciles resume/fork/rollback against the surviving rollout checkpoint.
- Fork recovery copies the referenced Artifact and TurnRecord dependencies from the source thread;
  rollback restores the surviving ancestor or clears checkpoint state when every checkpoint was
  rolled back. Recovery errors preserve the reconstructed conversation and are reported instead of
  preventing session startup.
- SessionSummary, five-generation QuarterSummary, LongTermMemory promotion, and memory citation
  consolidation remain later stages; they must build on the verified checkpoint ancestry rather
  than re-summarize raw rollout history.

### Three-layer state correction

This section is the authoritative model-visible architecture. Any later section that describes
ToolGroup or TurnRecord activity summaries is retained only as implementation history and must not
override this state model.

Compression is a state reduction, not an execution-log summary. After historical interaction is
removed, the projected state must let a fresh model choose the same next action without rereading
the removed tools in the normal case.

```text
QuarterState  stable cross-session facts, user rules, architecture, and verified milestones
SessionState  reusable understanding and decisions accumulated in the current session
ActiveState   precise objective, current facts, constraints, open questions, and next action
```

Raw ToolCall and ToolGroup data remains outside model context as immutable Artifact evidence. It is
available for selective recall but is not a fourth memory layer.

State ownership rules:

- A stable key has one current owner across Quarter, Session, and Active state.
- Active state is replaced as a complete current working set.
- Session entries are inserted or replaced by stable key.
- Quarter entries can only be promoted from memory that already existed in Session state before the
  current update; a model cannot create and promote a durable fact in one operation.
- Source facts and validation results require evidence. Source revision or content hash is retained
  when available so later source changes can invalidate the derived fact.
- Current source, Git diff, and commits remain authoritative for code. State stores intent,
  constraints, verified behavior, unresolved questions, and external validation rather than copying
  file contents or modification logs.
- Stale keys are explicitly forgotten before replacement. Historical versions remain immutable on
  disk but are absent from the current model projection.
- Normal requests filter all historical checkpoint fragments and append exactly one latest state
  projection at the request tail. If state projection fails, historical fragments remain visible as
  the conservative recovery path.

The model tool is `update_context_state`. ToolGroups only delimit raw history that may be removed;
they do not determine semantic memory content. The request writes a complete Active state, Session
upserts, stale keys to forget, and existing Session keys to promote to Quarter state.

Implemented in the current working stage:

- bounded typed Active, Session, and Quarter state;
- deterministic key merge, cross-layer uniqueness, evidence requirements, and delayed promotion;
- current-state checkpoint persistence through the existing immutable record and manifest chain;
- latest-only request projection with historical checkpoint fallback;
- runtime-owned content revisions for workspace-backed source facts, with stale facts removed before
  projection and before the next state merge;
- explicit checkpoint-generation Session boundaries and Quarter promotion only at each fifth
  Session boundary;
- core integration coverage that drives two consecutive checkpoints and asserts that the final
  model request contains exactly one latest state projection;
- legacy TurnRecord payload fields retained only for reading earlier manifests.
- semantic `ctx:Gxxxxxx/TRxxxxxx` checkpoint identities and deterministic
  `artifact:TRxxxxxx/Axxx` recall references, while SHA-256 remains integrity metadata only;
- a bounded model projection that preserves both three-layer state and legacy summary semantics
  without injecting manifest, artifact, or source-revision hashes;
- structured checkpoint lifecycle metadata through core protocol and app-server history, including
  labels, state-entry count, and the context-window chain;
- an adaptive TUI checkpoint history tree with narrow-window folding, raw transcript recovery, and
  Unicode Windows Terminal / ASCII legacy-console rendering;
- regenerated app-server JSON and TypeScript schema fixtures, with an opt-in Windows workflow export
  path that does not add work to normal push builds.

Runtime acceptance evidence:

- Windows workflow `31846792396` generated and uploaded the app-server schemas, and final-head
  workflow `31848095705` compiled and uploaded the Windows CLI successfully.
- Resumed-thread smoke `01a00283-6275-7e93-a1e0-59ca8cc77eac` created
  `ctx:G000001/TR000001`, restored its Active and Session state in the next request, and recalled
  `artifact:TR000001/A001` as `CONTEXT_CHECKPOINT_SMOKE_EVIDENCE` with exit status `0`.

## Problem

Codex currently treats compaction primarily as a whole-history replacement. Local compaction asks
the model for one summary, retains a bounded set of user messages, and replaces the active model
history. Remote compaction installs a provider-generated replacement history. In long-running
engineering tasks, either path can omit user decisions, unresolved failures, evidence, validation
results, or the exact next action. A lossy summary can therefore produce a confident but unstable
conclusion.

## Objective

Make controlled semantic checkpointing the primary context-management path while preserving all
baseline Codex activity: normal conversation, tool loops, final answers, resume/fork behavior,
rollout persistence, model switching, and provider fallback. Existing whole-history compaction must
remain available only as the final recovery path when a controlled checkpoint cannot complete.

## Required Model

```text
ToolCall -> Artifact
ActiveState -> SessionState -> QuarterState
```

- Runtime owns mechanical facts: IDs, paths, hashes, byte/token counts, timestamps, exit status,
  window state, persistence, reference validation, and atomic writes.
- The model owns semantic facts: task stage, why calls belong together, actual result, decisions,
  unresolved items, and when a stage is complete.
- Runtime must never invent semantic completion, and the model must never invent evidence IDs,
  paths, hashes, or validation status.

## Primary Runtime Path

1. Persist every tool invocation and result before model-history truncation or formatting loss.
2. Maintain one append-only active tail after an immutable frozen prefix.
3. Append an ephemeral runtime-computed `MEMORY_STATUS` to each model request tail.
4. Allow the model to replace effective state with `update_context_state` after a semantic work unit
   closes.
5. Validate state ownership, evidence, bounds, and ToolGroup ranges before atomically freezing it.
6. On the next model request, install a new context generation, filter historical state fragments
   from the model projection, and append the one latest Quarter/Session/Active state at the tail.
7. Preserve replaced raw interactions outside model context and expose exact recall by turn, group,
   call, or artifact ID.
8. Freeze SessionSummary generations without re-summarizing earlier generations. Consolidate every
   five completed sessions into a QuarterSummary. Promote only stable cross-quarter facts into
   LongTermMemory.
9. If checkpoint creation or validation fails, retain the original active context unchanged.
10. Invoke legacy local/remote whole-history compaction only when controlled settlement cannot
    recover enough space before the hard provider limit.

## Budget Semantics

- `context_usage` is calculated by Runtime from the usable budget after reserving space for a final
  answer, checkpoint output, tool-call completion, and provider overhead.
- No automatic whole-history compaction may run below 95% of that usable budget.
- The model may proactively settle a completed stage at any earlier point.
- At the hard checkpoint threshold, broad new exploration is blocked until a checkpoint succeeds or
  the legacy fallback is explicitly entered.
- The 95% threshold must not be interpreted as 95% of the provider's raw context window.

## Runtime Status

The status fragment must be bounded, computed by Runtime, placed at the request tail, and excluded
from the immutable cached prefix.

```xml
<MEMORY_STATUS>
session=S001 turn=T018
context_usage=73.4% tool_result_share=28.6%
tool_groups=3 open=1 settled=2
turn_record=pending last_checkpoint=T014
checkpoint_required=false tool=update_context_state
policy=retain_knowledge+retain_uncertainty+archive_process;no_next_action;settle_each_group
</MEMORY_STATUS>
```

Old status values are not rewritten into persisted history. A request receives only the current
ephemeral value, and the completed turn is reconciled once.

## Context State Tool Contract

```json
{
  "name": "update_context_state",
  "completedToolGroups": ["TG000018"],
  "toolGroupSettlements": [
    {
      "groupId": "TG000018",
      "disposition": "promote | keepOpen | archiveOnly",
      "stateKeys": ["source:checkpoint/runtime"],
      "openQuestions": []
    }
  ],
  "state": {
    "active": {
      "objective": "the current user-owned objective",
      "entries": [],
      "constraints": [],
      "openQuestions": [],
      "continuityHints": [
        "non-authoritative knowledge or evidence that may matter later; never an action"
      ]
    },
    "sessionUpserts": [],
    "removals": [],
    "quarterPromotions": []
  }
}
```

Runtime adds session, turn, group and call IDs; storage locations; hashes; token statistics; and
freeze metadata. A missing reference, hash mismatch, non-contiguous settlement, or false validation
claim rejects the checkpoint without deleting source context.

## Storage Requirements

The physical layout may reuse the existing rollout, state DB, and memories crates, but it must
provide these logical objects:

```text
memory/long-term.md
memory/active/S001.draft.md
memory/sessions/S001.md
memory/quarters/Q001.md
memory/turns/S001/T018.json
memory/artifacts/S001/T018/TG01/call-31.log
```

- Paths are derived deterministically from Runtime IDs.
- Frozen records are immutable. Corrections are appended as new records.
- Artifact manifests contain hashes, lengths, status, truncation provenance, and recall locations.
- Project memories are namespaced so unrelated repositories cannot contaminate one another.
- Secret-redaction and retention policy apply before summaries are uploaded to a model, without
  destroying the locally retained evidence required for exact recovery.

## Cache Requirements

- Keep system instructions, tool definitions, long-term memory, quarter summaries, session
  summaries, and frozen turn records in deterministic order.
- Keep `prompt_cache_key` stable for the project/thread generation.
- Dynamic statistics and random identifiers occur only in the active tail.
- Settling a tail may cause one controlled cache miss from the first changed token. The newly frozen
  prefix must remain byte/token stable on subsequent requests.
- Record provider-reported cached input tokens where available. Do not claim a cache hit when the
  provider does not report one.

## Recall And Quality Rules

- Recall order is TurnRecord, then ToolGroup excerpt, then full Artifact.
- Unresolved errors, unique evidence, and content still requiring line-by-line analysis are marked
  `requiresRecall` and cannot be discarded from the active reasoning set.
- User decisions, failure causes, validation conclusions, effective source knowledge, and explicit
  source coverage are mandatory when they exist. Compression never creates or selects a next action.
- A missing artifact or hash mismatch marks dependent summaries untrusted; Runtime never fills the
  gap by inference.
- Track original tokens, summary tokens, provider-reported cached tokens, recall count, rejected
  checkpoints, fallback count, and omission corrections per turn.

## Existing Codex Foundations To Reuse

- `core/src/session/context_window.rs`: authoritative context-window status.
- `core/src/context/token_budget_context.rs`: bounded contextual fragments and window metadata.
- `core/src/compact_token_budget.rs`: explicit context-window transition.
- `core/src/session/mod.rs`: rollout persistence and replacement-history installation.
- `core/src/session/rollout_reconstruction.rs`: deterministic resume from replacement history plus
  rollout suffix.
- `core/src/client.rs`: stable `prompt_cache_key` support.
- `memories/read` and `memories/write`: rollout summaries, consolidation, citations, and memory
  workspace handling.

The current `core/src/compact.rs` and `core/src/compact_remote.rs` implementations are fallback
adapters, not the owner of the new primary checkpoint model.

## Verified Source Map

This section records observed code ownership so resumed work starts from exact files and symbols
instead of repeating repository-wide discovery. Recheck an entry only when compilation or observed
behavior contradicts it.

| Concern | Verified owner and symbol | Observed behavior and required change |
| --- | --- | --- |
| Turn loop | `core/src/session/turn.rs` (`run_turn`) | The follow-up branch currently evaluates automatic compaction before the next sampling request. Consume and install a pending checkpoint before that legacy decision. |
| Request construction | `core/src/session/turn.rs` (`run_sampling_request`, `build_prompt`, `built_tools`) | Prompt input is selected from the supplied initial input or `ContextManager::for_prompt`, then tools and the prompt are built. Append one ephemeral status fragment here and keep it out of returned/persisted history. |
| Tool completion boundary | `core/src/tools/registry.rs` (`handle_any_tool`) | `tool.handle` currently returns before post-tool processing. Capture both the invocation and the success/error result at this boundary before ContextManager truncation, without changing the tool outcome on capture failure. |
| Generic tool output | `tools/src/tool_output.rs` (`ToolOutput`) | `to_response_item` is the shared model-facing conversion. Add a default archival payload method here; override it only where a handler owns a less lossy result. |
| History storage | `core/src/context_manager/history.rs` (`record_items`, `process_item`, `raw_items`, `for_prompt`, `replace`) | `record_items` applies truncation, while `raw_items` exposes the installed history and `for_prompt` normalizes a request copy. Projection must be calculated against installed history and must preserve every open-tail item. |
| Replacement installation | `core/src/session/mod.rs` (`replace_compacted_history`, `recompute_token_usage`, `reference_context_item`) | This is the existing atomic ordering boundary for live history plus rollout replacement records. Reuse it rather than adding a second reconstruction path. |
| Resume reconstruction | `core/src/session/rollout_reconstruction.rs` (`reconstruct_history_from_rollout`, `finalize_active_segment`) | The newest `CompactedItem.replacement_history` is used verbatim and later rollout items are replayed as a suffix. Optional checkpoint metadata can extend this without changing old rollout behavior. |
| Rollout protocol | `protocol/src/protocol.rs` (`CompactedItem`) | The item already stores `message`, optional `replacement_history`, and context-window chain IDs. Add optional checkpoint metadata with serde defaults so old rollouts remain readable. |
| Legacy local fallback | `core/src/compact.rs` (`run_compact_task_inner_impl`) | It builds a summary and replaces the whole active history. Retain it only as an explicitly reasoned fallback or manual `/compact` path. |
| Legacy remote fallback | `core/src/compact_remote.rs` (`run_remote_compact_task_inner_impl`) | It installs provider-produced replacement history through the same Session method. Retain it only as an observable provider fallback. |
| Pressure calculation | `core/src/session/context_window.rs` (`context_window_token_status`) | This is the authoritative source for active usage, scoped limits, full-window limits, and tokens remaining. Derive usable-budget pressure here rather than estimating it in the model. |
| Runtime ownership | `core/src/state/service.rs` (`SessionServices`) and `core/src/session/session.rs` (`Session::new`) | Store one per-thread `CheckpointRuntime` in services and construct it from a deterministic host-local root. Startup failure must produce a degraded runtime, not fail Session creation. |
| Tool registration | `core/src/tools/spec_plan.rs` (`add_core_utility_tools`, `build_tool_specs_and_registry`) | Register `update_context_state` and recall handlers here. Pressure-based visibility belongs in the tool-plan policy, not in individual handlers. |
| Context fragment contract | `context-fragments/src/fragment.rs` (`ContextualUserFragment`) | `into_response_input_item` creates a bounded role-bearing item. The status implementation belongs under `core/src/context/` and is appended only to the mutable request tail. |
| Existing tests | `core/src/session/rollout_reconstruction_tests.rs` and `core/src/session/tests.rs` | Existing coverage proves replacement history is restored verbatim. Extend these tests for optional metadata and checkpoint generation recovery rather than introducing a parallel test harness. |

### Verified Control Flow

1. `run_turn` clones normalized history and calls `run_sampling_request`.
2. `run_sampling_request` builds the tool router and prompt, then retries the same request internally.
3. Tool outputs pass through `handle_any_tool` before their response items are recorded into history.
4. The follow-up branch in `run_turn` is the first safe point after tool output rollout persistence.
5. `replace_compacted_history` persists `RolloutItem::Compacted` and replaces live history.
6. Resume reconstructs from the persisted replacement history and replays the remaining rollout suffix.

### Resume Checklist

- Read this document and `AGENTS_NAVIGATION.md` first.
- Inspect only the exact owner or symbol above when its recorded relation no longer holds.
- Do not begin with a whole-repository search for compaction, context, or tool handling.
- Treat Project AGENT workflow `31819232055` as a separate TUI delivery; it does not validate the
  checkpoint runtime.
- The checkpoint implementation workflow must run the focused matrix under **CI Execution** and must
  not add unrelated workspace-wide gates.

## Semantic References, Compact Recovery, And TUI History

The next delivery stage must treat content hashes as integrity metadata, not as the primary identity
shown to the model or user. Long SHA-256 values remain authoritative for immutable storage,
deduplication, and verification, but normal state, recall, diagnostics, and TUI surfaces use stable
semantic references.

### Semantic Reference Layer

- Checkpoints use a hierarchical reference such as `ctx:S001/G001/TR001`; Session, Generation, and
  TurnRecord components remain independently addressable.
- State entries use typed references such as `fact:source/source.smoke`,
  `decision:architecture/checkpoint-projection`, and `open:validation/cold-resume`.
- Recallable tool evidence uses a short deterministic reference such as `evidence:TG003/C001/output`.
- Entries carry bounded category labels for filtering and projection. Labels are Runtime-validated;
  the model does not invent storage paths, hashes, or sequence numbers.
- The manifest owns the semantic-reference-to-content-hash mapping. Default model projections and
  TUI rows omit full hashes; detail and integrity views may reveal them on demand.
- Existing content-addressed paths remain unchanged so this layer does not weaken verification or
  require rewriting immutable artifacts.

### Compact Recovery Projection

Before rendering `<CONTEXT_CHECKPOINT>`, Runtime deterministically reduces the durable state:

1. Merge entries by typed semantic key and keep only the latest valid revision.
2. Remove superseded, resolved, or source-invalidated entries from the active projection while
   retaining their immutable records for history and recall.
3. Preserve user decisions, active constraints, verified facts, unresolved failures, open questions,
   and the next direct action.
4. Replace repeated artifact hashes and evidence arrays with grouped semantic recall references.
5. Keep settled tool process and raw results outside model context unless an entry is marked as
   requiring recall. Open reasoning tails remain visible until settled.
6. Apply deterministic per-category and total byte/token caps. Overflow remains externally
   recallable and is represented by a bounded omission index, never silently discarded.
7. Serialize the same logical state in the same order so the frozen prefix remains cacheable.

The full manifest, TurnRecord, tool calls, outputs, and integrity hashes remain recoverable. Compact
projection changes model-visible recovery cost, not evidence retention.

### Selective Artifact Recall

- `recall_checkpoint_artifact` defaults to an outline that returns line counts, bounded section
  previews, approximate section sizes, and selectable line ranges without returning artifact text.
- `mode=lines` returns only an explicit 1-based line window, with a hard line cap, byte cap, and
  `nextStartLine` when the requested window cannot fit.
- `mode=search` scans the verified artifact but returns only bounded keyword matches, columns,
  excerpts, and a small number of neighboring lines. It never injects the full matching artifact.
- `mode=prefix` remains only as an explicit compatibility path. Normal model behavior must inspect
  the outline first and then select a line range or keyword query.
- Hash verification remains mandatory before outline, line, or search results are produced. These
  views reduce model-context cost; they do not weaken immutable Artifact evidence.

### Compression Philosophy: Three Checks And One Immutable History

Compression is a projection operation, not a rewrite of history and not a narrative of actions the
model performed. Before settling an item, the model and Runtime apply three checks:

1. **Validity:** whether the fact, constraint, decision, question, or next action is still effective,
   completed, invalidated, or superseded.
2. **Provenance:** which user instruction, source revision, tool evidence, or earlier state entry owns
   it, and which later record explicitly supersedes it.
3. **Dependency:** whether future reasoning can safely proceed without the item. Important unresolved
   questions, failure causes, and validation gaps remain active rather than disappearing for lack of
   a confirmed answer.

One history remains immutable: user decisions, externally effective actions, checkpoint ancestry,
and raw Artifact evidence are append-only. Compression may remove them from the active model view but
must never rewrite or physically delete them. A correction or revocation is another record linked to
the earlier node.

Policy consequences:

- Bare forgetting is forbidden for protected rules, decisions, and source knowledge. A removal
  requires explicit user revocation or a `supersededBy` reference; source invalidation is performed
  automatically from the verified source revision.
- `continuityHints` is the model's bounded, non-authoritative place to preserve knowledge or evidence
  that may matter later. It must never contain an action, commitment, or selected plan.
- Artifact entries require a semantic title, source/tool identity, size, and recall hint in addition
  to their stable reference and integrity hash.
- Before freezing a checkpoint, the Runtime should expose a bounded diff: retained, archived,
  superseded, and rejected removals, including the evidence for every protected-state transition.

### TUI Observability And History

- Normal conversation shows concise lifecycle events for collecting, validating, preparing,
  installing, restoring, invalidating, and recalling checkpoint state.
- Events show semantic checkpoint references, category labels, state, generation transition, and
  measured context reduction. Raw hashes are relegated to the detail view.
- A dedicated history tree presents `Session -> Generation -> TurnRecord -> state entries/evidence`
  without treating checkpoint generations as sub-agents.
- Selecting an entry opens a separate detail view with its summary, source revision, trust state,
  originating tool groups, and recallable evidence.
- Read-only history inspection and artifact recall must never mutate the active conversation.
- Restore, rollback, and fork are separate explicit actions. Their confirmation surface states which
  thread/window will change and preserves the current checkpoint before mutation.
- Resume and restoration failures remain visible on the corresponding tree node instead of being
  reduced to a transient toast or raw backend error.
- The TUI consumes structured lifecycle events; it does not infer checkpoint state by parsing text
  messages or hashes.

### Adaptive Folding And Windows Rendering

- Build the history surface from semantic nodes, not rendered text lines. Session, Generation,
  TurnRecord, state entry, tool group, and evidence nodes retain stable identity across refresh,
  resume, and resize.
- Keep the current generation, active work, failures, unresolved entries, and the selected path
  expanded. Fold completed older generations, settled tool groups, and repeated evidence by default.
- Collapse consecutive calls belonging to one settled ToolGroup into one row with status, call count,
  duration, and context cost. Load individual calls and raw output only when that node is opened.
- Manual expand/collapse choices override automatic folding and are persisted by semantic reference.
  Incoming events must not steal focus, collapse the selected branch, or reset scroll position.
- Adapt folding to viewport pressure without hiding failures, user decisions, active constraints,
  retained knowledge, or continuity hints. A bounded visible-row model virtualizes large trees and avoids rendering off-screen
  history.
- Keep thread identity separate from live worker/process state. A resumed or completed thread remains
  navigable from its persisted checkpoint index even when no worker is active.
- Load the checkpoint index first on resume and fetch TurnRecord, state-entry, and artifact details
  lazily. Opening history must not deserialize or inject the entire rollout into model context.
- Detect Windows terminal capabilities and select a rendering profile. Use box-drawing glyphs only
  where width and VT behavior are reliable; provide an aligned ASCII tree for legacy ConHost or
  incompatible terminals.
- Avoid emoji and ambiguous-width symbols in structural columns. Calculate CJK text width through the
  existing TUI width/wrapping utilities and reserve stable columns for status and tree indentation.
- Coalesce rapid lifecycle updates and redraw only changed visible rows. Resize, alternate-screen
  restoration, and resume must preserve selection, expansion state, and scroll anchor.
- Do not depend on OSC features, true color, or terminal-specific cursor behavior for correctness.
  Color and richer glyphs are progressive enhancements over a complete plain-text representation.
- Add snapshots for Unicode and ASCII profiles at narrow and wide Windows-oriented viewports,
  including CJK labels, deep nesting, long tool groups, resize, resume, failure, and restored state.

### Required Runtime Acceptance

- Verify a checkpoint installation and projection transition in one live turn.
- Restart the process and resume the same thread, proving semantic IDs and projected state survive.
- Install multiple generations and prove only the latest applicable checkpoint is injected while
  earlier generations remain navigable.
- Change a referenced source file and prove the stale fact is invalidated in both projection and UI.
- Recall evidence through a semantic reference and verify its underlying content hash.
- Exercise read-only history, restore, rollback, and fork independently in the TUI.
- Exercise automatic folding, manual expansion persistence, lazy detail loading, and selection
  stability across resize and resume.
- Verify both Windows Terminal and legacy-compatible rendering profiles without overlap, width drift,
  flicker-inducing full-tree rebuilds, or inaccessible history nodes.
- Snapshot all user-visible checkpoint lifecycle and history-tree states.

## Acceptance Criteria

1. A long tool-heavy task crosses multiple context generations without losing recorded user
   decisions, unresolved errors, actual validation results, or the exact next action.
2. Completed and partially completed ToolGroups survive process restart and thread resume with the
   same IDs, evidence links, and trust state.
3. A failed or rejected checkpoint leaves the original model-visible tail and all artifacts intact.
4. Only a contiguous completed tail segment can be settled; open groups remain model-visible.
5. Normal tool execution and final answers continue when no checkpoint is needed.
6. Manual `new_context`, model changes, resume, fork, rollback, and remote providers have defined
   behavior and do not bypass persistence.
7. Whole-history compaction is observable as a fallback event and never occurs below the configured
   usable-budget threshold.
8. Every summary statement used for later decisions can be traced to an existing immutable record
   or artifact.
9. Cache telemetry demonstrates that frozen prefixes remain stable after the one request that
   installs a new generation, where the provider exposes cache accounting.
10. GitHub workflow coverage includes checkpoint success, validation rejection, restart/resume,
    fallback, and model/provider transition paths.

## Out Of Scope

- Claiming or guaranteeing provider-internal cache behavior that the API does not expose.
- Claiming that a provider will never enforce its own context limit or internal truncation.
- Treating the existing cross-thread memory consolidation pipeline as a substitute for active
  in-session checkpointing.
- Inferring ToolGroup purpose solely from adjacent calls without a model-authored semantic record.

## Implementation Architecture

### Ownership Boundaries

The implementation must not grow `codex-core` into the owner of another storage and memory domain.
Use three layers:

1. `codex-context-checkpoint` owns IDs, records, validation, deterministic paths, artifact manifests,
   projection planning, recall, and file persistence. It depends on protocol and utility crates, but
   never on `codex-core`.
2. `codex-core` owns live Session integration: request-tail status injection, tool registration,
   grouping live tool calls, pending checkpoint installation, and fallback selection.
3. `codex-memories-*` consumes completed immutable SessionSummary records for quarter and long-term
   consolidation. It does not own active-turn settlement.

The file-backed checkpoint store is authoritative. State DB rows may index records and jobs, but a
missing or stale DB index cannot invalidate an otherwise valid hashed file record.

### Naming Boundary

The design document's `S001` is a checkpoint generation, not the existing Codex process Session.
Use `CheckpointGenerationId` in Rust and render it as `S001` on disk. This avoids mixing thread,
agent session, rollout session, and context-generation identities.

### Compatibility Strategy

Do not introduce a second rollout reconstruction algorithm. Extend `CompactedItem` with an optional
checkpoint metadata field while continuing to persist `replacement_history`:

```rust
// Preserve the existing derives and compatibility deserializer.
pub struct CompactedItem {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_history: Option<Vec<ResponseItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<ContextCheckpointRolloutMetadata>,
    // Existing window identity fields remain unchanged.
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContextCheckpointRolloutMetadata {
    pub checkpoint_id: String,
    pub generation_id: String,
    pub turn_record_id: String,
    pub manifest_sha256: String,
    pub fallback_kind: Option<ContextFallbackKind>,
}
```

Older binaries ignore the additional field and still resume from `replacement_history`. New
binaries use the metadata to restore checkpoint state and verify the external manifest. Existing
compaction items with no metadata are treated as legacy compaction records.

## Code Layout

### New Crate

```text
codex-rs/context-checkpoint/
  Cargo.toml
  BUILD.bazel
  src/lib.rs                 narrow public API and crate documentation
  src/ids.rs                 typed deterministic IDs and parsing
  src/model.rs               ToolCall, ToolGroup, TurnRecord, summaries
  src/budget.rs              usable-budget and pressure calculations
  src/artifact.rs            artifact payload, manifest, hashing
  src/store.rs               atomic file store using AbsolutePathBuf
  src/validation.rs          evidence and settlement validation
  src/projection.rs          stable-prefix plus active-tail projection plan
  src/recall.rs              bounded staged recall
  src/metrics.rs             checkpoint outcome measurements
  src/store_tests.rs
  src/validation_tests.rs
  src/projection_tests.rs
```

Each implementation module should remain below 500 lines. Public exports stay in `lib.rs`; storage
helpers and wire conversion details remain private.

### Core Integration

```text
codex-rs/core/src/context_checkpoint/
  mod.rs                     thin Session-facing facade
  runtime.rs                 per-thread live checkpoint state
  request.rs                 MEMORY_STATUS snapshot and tool policy
  settlement.rs              pending checkpoint lifecycle
  projection.rs              ResponseItem projection adapter

codex-rs/core/src/context/checkpoint_status.rs
codex-rs/core/src/tools/handlers/checkpoint.rs
codex-rs/core/src/tools/handlers/recall_checkpoint.rs
codex-rs/core/src/tools/handlers/recall_checkpoint_spec.rs
codex-rs/core/src/session/context_checkpoint.rs
```

The new `session/context_checkpoint.rs` contains `impl Session` methods for checkpoint operations.
Only module declarations and existing construction calls are added to large orchestration files.

### Existing Files With Targeted Changes

| File | Required change |
| --- | --- |
| `codex-rs/tools/src/tool_output.rs` | Add a loss-aware archival payload method with a model-output default. |
| `codex-rs/core/src/tools/registry.rs` | Persist invocation/result before returning `AnyToolResult`, including failures. |
| `codex-rs/core/src/tools/spec_plan.rs` | Register checkpoint and recall tools; apply checkpoint-only visibility. |
| `codex-rs/core/src/session/session.rs` | Construct and retain one `CheckpointRuntime` in `SessionServices`. |
| `codex-rs/core/src/session/turn.rs` | Append ephemeral status before `build_prompt`; install pending checkpoints after tool recording. |
| `codex-rs/core/src/session/context_window.rs` | Calculate usable-budget pressure and fallback threshold. |
| `codex-rs/core/src/compact_token_budget.rs` | Route normal window changes through controlled checkpointing. |
| `codex-rs/core/src/compact.rs` | Mark local whole-history replacement as an observable fallback. |
| `codex-rs/core/src/compact_remote.rs` | Mark provider compaction as an observable fallback. |
| `codex-rs/protocol/src/protocol.rs` | Add optional checkpoint metadata to `CompactedItem`. |
| `codex-rs/memories/write/src/phase1.rs` | Prefer verified SessionSummary records over re-summarizing raw rollout content. |
| `codex-rs/memories/write/src/phase2.rs` | Add fixed five-generation quarter consolidation. |
| `codex-rs/memories/read/src/citations.rs` | Resolve checkpoint, group, call, and artifact citations. |
| `codex-rs/config/src/types.rs` | Add reserve, retention, and fallback settings without opaque bool call sites. |
| `codex-rs/features/src/lib.rs` | Add the controlled checkpoint feature and make it the normal path once complete. |
| `AGENTS_NAVIGATION.md` | Record the new feature owner, entry points, persistence boundary, and focused CI command. |

Workspace manifests, Bazel targets, `Cargo.lock`, and `MODULE.bazel.lock` must be updated in the same
change where required.

## Core Rust Types

The following is the intended API shape, not placeholder pseudocode to be left unimplemented.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CheckpointGenerationId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TurnRecordId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ToolGroupId(u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactId(String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub artifact_id: ArtifactId,
    pub sha256: String,
    pub byte_len: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub call_id: String,
    pub tool_name: String,
    pub group_id: ToolGroupId,
    pub input: ArtifactRef,
    pub output: ArtifactRef,
    pub outcome: ToolCallOutcome,
    pub truncation: TruncationProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallOutcome {
    Success,
    ToolError { message: String },
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolGroupRecord {
    pub group_id: ToolGroupId,
    pub calls: Vec<ToolCallRecord>,
    pub state: ToolGroupState,
    pub requires_recall: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolGroupState {
    Open,
    Settled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnRecord {
    pub record_id: TurnRecordId,
    pub generation_id: CheckpointGenerationId,
    pub completed_groups: Vec<ToolGroupId>,
    pub summary: String,
    pub evidence: Vec<EvidenceRef>,
    pub changes: Vec<String>,
    pub validation: Vec<String>,
    pub decisions: Vec<String>,
    pub open_items: Vec<String>,
    pub next_action: String,
    pub correction_of: Option<TurnRecordId>,
}
```

Use enums for pressure and tool visibility rather than positional boolean arguments:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointPressure {
    Normal,
    Advisory,
    Required,
    FallbackRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointToolPolicy {
    Normal,
    CheckpointOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStatusSnapshot {
    pub generation_id: CheckpointGenerationId,
    pub turn_id: String,
    pub context_usage_basis_points: u16,
    pub usage_source: UsageSource,
    pub tool_result_share_basis_points: u16,
    pub open_groups: usize,
    pub settled_groups: usize,
    pub last_checkpoint: Option<TurnRecordId>,
    pub pressure: CheckpointPressure,
}
```

Provider token usage is authoritative for total usage when present. Per-item tool-result share is
necessarily tokenizer-derived because providers expose aggregate cached/input usage, not token
counts for each history item. `usage_source` makes that distinction explicit.

## Runtime API

Keep filesystem I/O outside the main Session mutex. The runtime owns a short-lived internal lock and
an immutable store handle:

```rust
pub struct CheckpointRuntime {
    store: CheckpointStore,
    state: tokio::sync::Mutex<CheckpointState>,
}

impl CheckpointRuntime {
    pub async fn prepare_request(
        &self,
        input: &[ResponseItem],
        usage: ContextUsageSnapshot,
    ) -> Result<PreparedCheckpointRequest, CheckpointError>;

    pub async fn record_tool_result(
        &self,
        invocation: ToolInvocationRecord,
        result: ToolResultRecord,
    ) -> ArtifactCaptureOutcome;

    pub async fn prepare_summary(
        &self,
        request: UpdateSummaryRequest,
    ) -> Result<PendingCheckpoint, CheckpointError>;

    pub async fn take_pending_checkpoint(&self) -> Option<PendingCheckpoint>;

    pub async fn mark_installed(
        &self,
        installed: InstalledCheckpoint,
    ) -> Result<(), CheckpointError>;

    pub async fn recall(
        &self,
        request: RecallRequest,
    ) -> Result<RecallResult, CheckpointError>;
}

#[derive(Debug)]
pub enum ArtifactCaptureOutcome {
    Recorded(ToolCallRecord),
    Degraded(ArtifactCaptureFailure),
}
```

`CheckpointStore` is a concrete type until a second real storage implementation exists. Host-local
roots use `AbsolutePathBuf`. Atomic writes use a same-directory temporary file, flush, and rename;
the manifest is written last so a manifest always refers only to complete artifacts.

## Tool Output Archival

Extend the generic tool-output contract without depending on the checkpoint crate:

```rust
#[derive(Debug, Clone, Serialize)]
pub enum ToolOutputArtifact {
    ResponseItem(ResponseInputItem),
    Json(serde_json::Value),
    Bytes {
        media_type: String,
        data: Vec<u8>,
    },
}

pub trait ToolOutput: Send {
    fn log_preview(&self) -> String;
    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem;

    fn artifact_payload(
        &self,
        call_id: &str,
        payload: &ToolPayload,
    ) -> ToolOutputArtifact {
        ToolOutputArtifact::ResponseItem(self.to_response_item(call_id, payload))
    }
}
```

The default archives exactly what core receives before ContextManager truncation. Shell, exec, MCP,
image, and other handlers that possess a more complete result override `artifact_payload` and mark
whether model-facing formatting omitted content.

`handle_any_tool` attempts to persist both success and failure before returning to the model. A
checkpoint-storage failure must not change the unrelated tool's outcome:

```rust
let result = tool.handle(invocation.clone()).await;
let capture = invocation
    .session
    .record_checkpoint_tool_result(&invocation, result.as_deref())
    .await;
invocation.session.note_checkpoint_capture(capture).await;
let output = result?;
```

The example expresses ordering. The production call handles the success and error references
exhaustively rather than assuming a conversion between their types.

## Baseline Activity Guarantee

The checkpoint subsystem is an optimization and integrity layer around existing activity. It must
not become a new availability dependency for ordinary Codex work.

- A shell, MCP, file, image, or other unrelated tool keeps its original success/error result if
  checkpoint artifact capture fails.
- Capture failure moves the runtime to `Degraded`, emits one bounded event, and records the reason in
  rollout when persistence remains available.
- `update_context_state` is rejected while required evidence is degraded; it never freezes an incomplete
  record as trusted.
- At normal pressure the conversation continues with the original history intact.
- At fallback pressure the existing local/remote compaction path runs with the degradation reason.
- Checkpoint file I/O never occurs while holding the main Session state lock.
- Startup failure of the checkpoint store leaves normal conversation, manual `/compact`, resume,
  fork, rollback, and tool execution usable.
- Panics, repeated warnings, and retry loops in checkpoint code are not allowed to take down or stall
  the active turn.

## ToolGroup Formation

- Runtime creates one ToolGroup for each assistant sampling step that emits one or more tool calls.
- Parallel calls emitted by the same response share the group.
- A later sampling step receives a new group ID.
- Runtime does not infer the semantic purpose. `update_context_state` supplies the meaning and may settle
  one or more contiguous completed groups.
- Groups with in-flight calls, unresolved unique evidence, or `requiresRecall=true` cannot be
  removed from the active tail.

This gives Runtime mechanical boundaries while preserving the model's responsibility for semantic
stage completion.

## Request Construction

`run_sampling_request` currently selects either initial input or `history.for_prompt(...)`, then calls
`build_prompt`. Insert one bounded preparation call between those operations:

```rust
let prompt_input = if let Some(input) = initial_input.take() {
    input
} else {
    sess.clone_history()
        .await
        .for_prompt(&turn_context.model_info.input_modalities)
};
let prepared = sess
    .prepare_checkpoint_request(turn_context.as_ref(), prompt_input)
    .await?;
let router = built_tools_with_policy(
    sess.as_ref(),
    step_context.as_ref(),
    &cancellation_token,
    prepared.tool_policy,
).await?;
let prompt = build_prompt(prepared.input, router.as_ref(), turn_context, base_instructions);
```

`checkpoint_status.rs` implements `ContextualUserFragment` and renders the final ephemeral developer
item. It has a hard byte/token cap and is never written into ContextManager or rollout history.
Retries rebuild only the mutable request tail; frozen history items remain byte-for-byte stable.

## Context State Tool Lifecycle

The handler validates JSON and prepares a checkpoint, but does not replace history inside tool
dispatch:

```rust
let pending = invocation
    .session
    .checkpoint_runtime()
    .prepare_summary(request)
    .await
    .map_err(checkpoint_error_for_model)?;
invocation.session.set_pending_checkpoint(pending).await;
```

After the update-summary tool call and output have been recorded in rollout, `run_turn` consumes the
pending checkpoint before the next follow-up sampling request:

```rust
if needs_follow_up
    && let Some(pending) = sess.take_pending_checkpoint().await
{
    sess.install_checkpoint_generation(turn_context.as_ref(), pending)
        .await?;
    continue;
}
```

Installation performs these operations in order:

1. Revalidate referenced groups and artifact hashes.
2. Write the immutable TurnRecord and updated draft.
3. Build a projection preserving the exact frozen prefix and all open tail items.
4. Append the new bounded checkpoint fragment at the former settled-tail boundary.
5. Persist `CompactedItem` with replacement history and checkpoint metadata.
6. Replace live ContextManager history.
7. Recompute token usage and mark the manifest installed.

Failure before step 5 leaves live history unchanged. Failure after rollout persistence is repaired on
resume by the persisted replacement history and manifest state.

## Projection Invariants

```text
immutable initial prefix
immutable long-term / quarter / session records
immutable frozen TurnRecords
current world-state update
open active tail
ephemeral MEMORY_STATUS
```

- A checkpoint may replace only a contiguous settled portion at the beginning of the active tail.
- It may not remove an item from the middle while preserving later raw items as if the prefix were
  unchanged.
- Initial developer/tool definitions are not regenerated during ordinary settlement.
- World-state changes are appended as the newest bounded state item rather than inserted into an
  earlier frozen prefix.
- Corrections append a new record; they never edit a cached frozen record.

## Budget And Fallback Flow

```rust
match pressure {
    CheckpointPressure::Normal => continue_normally(),
    CheckpointPressure::Advisory => expose_checkpoint_tool_and_status(),
    CheckpointPressure::Required => restrict_to_checkpoint_recall_or_final_answer(),
    CheckpointPressure::FallbackRequired => run_legacy_compaction_with_event(),
}
```

The fallback transition requires a recorded reason such as no settleable groups, repeated validation
failure, artifact-store failure, incompatible model history, or provider hard rejection. A generic
error cannot silently select legacy compaction.

Manual `/compact` remains an explicit legacy operation for compatibility. Automatic compaction and
`new_context` use the controlled checkpoint path when the feature is enabled. Model/comp-hash
transitions checkpoint using the previous compatible model before switching; provider compaction is
used only if that controlled transition fails.

## Memory Consolidation

1. Each installed checkpoint closes one numbered Session generation; the next model request starts
   the next generation from the frozen state projection.
2. Session entries remain mutable by stable-key upsert within those generations, while every fifth
   generation exposes `quarter_consolidation_due=true` and permits promotion of pre-existing Session
   keys.
3. A Quarter promotion moves only already-established Session entries; entries created in the same
   update cannot be promoted.
4. Phase 2 receives only the new quarter plus the existing bounded LongTermMemory and emits an
   additive update/correction set.
5. LongTermMemory accepts stable user decisions, architecture constraints, and durable project
   facts; transient tool output and one-off failures remain in lower levels.

Existing memory citations are extended instead of replaced. Citation resolution verifies files and
hashes before returning content to the model.

## Configuration Shape

```rust
pub struct ContextCheckpointConfig {
    pub final_answer_reserve_tokens: i64,
    pub checkpoint_reserve_tokens: i64,
    pub provider_overhead_reserve_tokens: i64,
    pub advisory_usage_percent: u8,
    pub required_usage_percent: u8,
    pub fallback_usage_percent: u8,
    pub max_status_tokens: usize,
    pub max_turn_record_tokens: usize,
    pub artifact_retention: ArtifactRetention,
}

pub enum ArtifactRetention {
    KeepAll,
    KeepRecentGenerations { generations: usize },
}
```

Defaults enforce `advisory < required < fallback`, and `fallback` represents 95% of the reserved
usable budget. Invalid combinations fail config loading instead of being silently reordered.

## Focused Verification Matrix

### New Crate

- Deterministic IDs and cross-platform paths.
- Atomic artifact/manifest writes and interrupted-write recovery.
- Hash mismatch and missing-evidence rejection.
- Contiguous-settlement enforcement.
- Stable-prefix projection equality.
- Bounded recall escalation.

### Core Integration

- A tool-heavy turn calls `update_context_state`; the next request contains the frozen TurnRecord and omits
  only the settled raw groups.
- Open groups remain byte-for-byte in the next request.
- Invalid references return a tool error and leave history unchanged.
- Tool success and tool failure artifacts exist before their output is sent back to the model.
- `MEMORY_STATUS` appears once at the request tail and is absent from persisted rollout history.
- Restart/resume reconstructs the same replacement history and checkpoint generation.
- Fork and rollback select the correct checkpoint ancestor.
- Model switch uses controlled settlement before legacy fallback.
- Provider rejection records fallback reason and preserves artifacts.

### Memory Integration

- Five generation summaries create exactly one QuarterSummary.
- A second quarter does not re-summarize the first quarter's source sessions.
- Corrections append and supersede without mutating cited source lines.
- Missing or changed artifacts make dependent recall untrusted.

### CI Execution

Use GitHub workflows, not local compilation, for this workspace. The implementation workflow should
run formatting, the new crate tests, focused `codex-core` integration tests, protocol serialization
compatibility, and the Windows `codex.exe` build. Do not add unrelated workspace-wide gates to the
deployment path.

## Implementation Order

This order is for code dependency management, not for shipping incomplete user-visible behavior:

1. Add the domain/store crate and compatibility metadata.
2. Add artifact capture and per-thread runtime construction.
3. Add status fragment, tool specs, handlers, and grouping.
4. Add pending settlement and stable projection installation.
5. Route automatic pressure and `new_context` through the new path; retain explicit fallback.
6. Extend resume, fork, rollback, memory consolidation, and recall.
7. Enable the new path by default only when the complete verification matrix passes.

The deployable result must contain all seven steps. Intermediate commits may compile for review, but
must not be presented as the completed replacement runtime.
