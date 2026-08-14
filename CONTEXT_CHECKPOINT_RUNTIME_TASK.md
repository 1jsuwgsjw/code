# Controlled Context Checkpoint Runtime

## Status

Recorded for implementation after the current Project AGENT build is deployed and validated.
This document is the fixed task definition. Implementation must not silently narrow or replace
its objective.

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
ToolCall -> ToolGroup -> TurnRecord -> SessionSummary -> QuarterSummary -> LongTermMemory
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
4. Allow the model to close completed contiguous ToolGroups with `update_summary`.
5. Validate all references and atomically freeze the resulting TurnRecord.
6. On the next model request, install a new context generation containing the stable frozen prefix,
   the new immutable record, and the still-open activity tail.
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
checkpoint_required=false tool=update_summary
</MEMORY_STATUS>
```

Old status values are not rewritten into persisted history. A request receives only the current
ephemeral value, and the completed turn is reconciled once.

## Summary Tool Contract

```json
{
  "name": "update_summary",
  "scope": "turn | stage | session",
  "completedToolGroups": ["TG-018-01"],
  "summary": "purpose, essential process, and actual result",
  "evidence": ["existing file, symbol, record, or artifact references"],
  "changes": ["changes actually produced"],
  "validation": ["validation performed and its real result"],
  "decisions": ["user and engineering decisions"],
  "openItems": ["unfinished work and why it remains open"],
  "nextAction": "one directly executable next action"
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
- User decisions, failure causes, validation conclusions, and `nextAction` are mandatory fields.
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
