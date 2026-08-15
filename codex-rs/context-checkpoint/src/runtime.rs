use crate::ArtifactCaptureFailure;
use crate::ArtifactCaptureOutcome;
use crate::ArtifactId;
use crate::ArtifactLocator;
use crate::ArtifactRef;
use crate::CheckpointBudget;
use crate::CheckpointError;
use crate::CheckpointGenerationId;
use crate::CheckpointPressure;
use crate::CheckpointRolloutState;
use crate::CheckpointStore;
use crate::CheckpointToolPolicy;
use crate::ContextUsageSnapshot;
use crate::EvidenceRef;
use crate::InstalledCheckpoint;
use crate::MemoryStatusSnapshot;
use crate::PendingCheckpoint;
use crate::PreparedCheckpointRequest;
use crate::RecallRequest;
use crate::RecallResult;
use crate::ToolCallOutcome;
use crate::ToolCallRecord;
use crate::ToolGroupId;
use crate::ToolGroupRecord;
use crate::ToolGroupState;
use crate::ToolInvocationRecord;
use crate::ToolResultRecord;
use crate::TruncationProvenance;
use crate::TurnRecord;
use crate::TurnRecordId;
use crate::UpdateContextStateRequest;
use crate::evidence_bridge::attach_settled_group_artifacts;
use crate::settlement::validate_tool_group_settlements;
use crate::source_revision::invalidate_stale_source_facts;
use crate::source_revision::stamp_source_facts;
use crate::state::SESSIONS_PER_QUARTER;
use crate::state::apply_context_state_update;
use crate::state::update_evidence;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use tokio::sync::Mutex;

pub(crate) const MANIFEST_SCHEMA_VERSION: u32 = 1;
const MAX_RECALL_BYTES: usize = 256 * 1024;
const MAX_CHECKPOINT_RECORD_BYTES: usize = 32 * 1024;

pub struct CheckpointRuntime {
    pub(crate) store: CheckpointStore,
    budget: CheckpointBudget,
    pub(crate) state: Mutex<RuntimeState>,
    persist_lock: Mutex<()>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeState {
    pub(crate) manifest: RuntimeManifest,
    pub(crate) degraded_reason: Option<String>,
    pub(crate) manifest_sha256: Option<String>,
    last_tool_policy: CheckpointToolPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeManifest {
    pub(crate) schema_version: u32,
    pub(crate) generation_id: CheckpointGenerationId,
    pub(crate) next_turn_record_id: u64,
    pub(crate) next_tool_group_id: u64,
    pub(crate) active_sampling: Option<SamplingBoundary>,
    pub(crate) tool_groups: Vec<ToolGroupRecord>,
    pub(crate) turn_records: Vec<TurnRecord>,
    #[serde(default)]
    pub(crate) artifacts: Vec<ArtifactRef>,
    pub(crate) pending_checkpoint: Option<PendingCheckpoint>,
    pub(crate) last_installed: Option<InstalledCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SamplingBoundary {
    turn_id: String,
    history_start: usize,
    group_id: Option<ToolGroupId>,
}

impl RuntimeManifest {
    pub(crate) fn empty() -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            generation_id: CheckpointGenerationId::new(1),
            next_turn_record_id: 1,
            next_tool_group_id: 1,
            active_sampling: None,
            tool_groups: Vec::new(),
            turn_records: Vec::new(),
            artifacts: Vec::new(),
            pending_checkpoint: None,
            last_installed: None,
        }
    }
}

impl CheckpointRuntime {
    pub async fn load(store: CheckpointStore, budget: CheckpointBudget) -> Self {
        match Self::try_load(store.clone(), budget).await {
            Ok(runtime) => runtime,
            Err(error) => Self {
                store,
                budget,
                state: Mutex::new(RuntimeState {
                    manifest: RuntimeManifest::empty(),
                    degraded_reason: Some(error.to_string()),
                    manifest_sha256: None,
                    last_tool_policy: CheckpointToolPolicy::Normal,
                }),
                persist_lock: Mutex::new(()),
            },
        }
    }

    pub async fn try_load(
        store: CheckpointStore,
        budget: CheckpointBudget,
    ) -> Result<Self, CheckpointError> {
        let manifest = store
            .load_manifest::<RuntimeManifest>()
            .await?
            .unwrap_or_else(RuntimeManifest::empty);
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(CheckpointError::Storage(format!(
                "unsupported checkpoint manifest version {}",
                manifest.schema_version
            )));
        }
        let manifest_sha256 = store.current_manifest_sha256().await?;
        let runtime = Self {
            store,
            budget,
            state: Mutex::new(RuntimeState {
                manifest,
                degraded_reason: None,
                manifest_sha256,
                last_tool_policy: CheckpointToolPolicy::Normal,
            }),
            persist_lock: Mutex::new(()),
        };
        if runtime.state.lock().await.manifest_sha256.is_none() {
            runtime.persist().await?;
        }
        Ok(runtime)
    }

    pub fn store(&self) -> &CheckpointStore {
        &self.store
    }

    pub fn pressure(&self, usage: ContextUsageSnapshot) -> CheckpointPressure {
        self.budget.pressure(self.budget.usage_basis_points(usage))
    }

    pub async fn tool_policy(&self) -> CheckpointToolPolicy {
        self.state.lock().await.last_tool_policy
    }

    pub async fn rollout_state(&self) -> CheckpointRolloutState {
        let state = self.state.lock().await;
        CheckpointRolloutState {
            generation_id: state.manifest.generation_id,
            last_checkpoint: state
                .manifest
                .last_installed
                .as_ref()
                .map(|installed| installed.record_id),
            manifest_sha256: state.manifest_sha256.clone(),
        }
    }

    pub async fn prepare_request(
        &self,
        turn_id: impl Into<String>,
        history_len: usize,
        usage: ContextUsageSnapshot,
        tool_result_share_basis_points: u16,
        workspace_root: &Path,
    ) -> Result<PreparedCheckpointRequest, CheckpointError> {
        let turn_id = turn_id.into();
        {
            let mut state = self.state.lock().await;
            if let Some(previous) = state.manifest.active_sampling.take()
                && let Some(group_id) = previous.group_id
            {
                let group = state
                    .manifest
                    .tool_groups
                    .iter_mut()
                    .find(|group| group.group_id == group_id)
                    .ok_or_else(|| {
                        CheckpointError::InvalidRequest(format!(
                            "active tool group {group_id} is missing"
                        ))
                    })?;
                group.history_end = Some(history_len);
                group.state = ToolGroupState::Settled;
            }
            state.manifest.active_sampling = Some(SamplingBoundary {
                turn_id,
                history_start: history_len,
                group_id: None,
            });
        }
        if let Err(error) = self.persist().await {
            self.degrade(error.to_string()).await;
        }
        let mut prepared = self
            .prepared_status(usage, tool_result_share_basis_points)
            .await;
        if let Some(record) = prepared.record.as_mut() {
            invalidate_stale_source_facts(&mut record.state, workspace_root).await;
        }
        Ok(prepared)
    }

    pub async fn record_tool_result(
        &self,
        invocation: ToolInvocationRecord,
        result: ToolResultRecord,
    ) -> ArtifactCaptureOutcome {
        let call_id = invocation.call_id.clone();
        let input = match self
            .store
            .write_artifact(invocation.media_type, &invocation.payload)
            .await
        {
            Ok(input) => input,
            Err(error) => return self.capture_failure(call_id, error).await,
        };
        let output = match self
            .store
            .write_artifact(result.media_type, &result.payload)
            .await
        {
            Ok(output) => output,
            Err(error) => return self.capture_failure(call_id, error).await,
        };

        let record = {
            let mut state = self.state.lock().await;
            if let Some(existing) = state
                .manifest
                .tool_groups
                .iter()
                .flat_map(|group| &group.calls)
                .find(|call| call.call_id == call_id)
            {
                return ArtifactCaptureOutcome::Recorded(existing.clone());
            }
            let Some(active) = state.manifest.active_sampling.clone() else {
                drop(state);
                return self
                    .capture_failure(
                        call_id,
                        CheckpointError::InvalidRequest(
                            "tool result arrived outside a prepared sampling step".to_string(),
                        ),
                    )
                    .await;
            };
            let group_id = match active.group_id {
                Some(group_id) => group_id,
                None => {
                    let group_id = ToolGroupId::new(state.manifest.next_tool_group_id);
                    state.manifest.next_tool_group_id =
                        state.manifest.next_tool_group_id.saturating_add(1);
                    state.manifest.tool_groups.push(ToolGroupRecord {
                        group_id,
                        turn_id: active.turn_id,
                        history_start: active.history_start,
                        history_end: None,
                        calls: Vec::new(),
                        state: ToolGroupState::Open,
                        requires_recall: false,
                    });
                    state
                        .manifest
                        .active_sampling
                        .as_mut()
                        .expect("active sampling was checked above")
                        .group_id = Some(group_id);
                    group_id
                }
            };
            let record = ToolCallRecord {
                call_id,
                tool_name: invocation.tool_name,
                group_id,
                input,
                output,
                outcome: result.outcome,
                truncation: result.truncation,
            };
            for artifact in [&record.input, &record.output] {
                if !state
                    .manifest
                    .artifacts
                    .iter()
                    .any(|existing| existing.artifact_id == artifact.artifact_id)
                {
                    state.manifest.artifacts.push(artifact.clone());
                }
            }
            let group = state
                .manifest
                .tool_groups
                .iter_mut()
                .find(|group| group.group_id == group_id)
                .expect("active tool group was created above");
            group.requires_recall |= record.truncation != TruncationProvenance::Complete
                || matches!(&record.outcome, ToolCallOutcome::ToolError { .. });
            group.calls.push(record.clone());
            record
        };

        match self.persist().await {
            Ok(_) => ArtifactCaptureOutcome::Recorded(record),
            Err(error) => self.capture_failure(record.call_id, error).await,
        }
    }

    pub async fn prepare_context_state(
        &self,
        request: UpdateContextStateRequest,
        workspace_root: &Path,
    ) -> Result<PendingCheckpoint, CheckpointError> {
        let snapshot = {
            let state = self.state.lock().await;
            if let Some(reason) = &state.degraded_reason {
                return Err(CheckpointError::Degraded(reason.clone()));
            }
            if state.manifest.pending_checkpoint.is_some() {
                return Err(CheckpointError::InvalidRequest(
                    "a checkpoint is already pending installation".to_string(),
                ));
            }
            state.manifest.clone()
        };
        let mut current_state = snapshot
            .turn_records
            .last()
            .map(|record| record.state.clone())
            .unwrap_or_default();
        invalidate_stale_source_facts(&mut current_state, workspace_root).await;
        let mut context_state =
            apply_context_state_update(&current_state, &request.state, snapshot.generation_id)?;
        stamp_source_facts(&mut context_state, workspace_root).await?;
        let selected = validate_group_selection(&snapshot, &request)?;
        validate_tool_group_settlements(
            &selected,
            &request.tool_group_settlements,
            &context_state,
        )?;
        let mut evidence = update_evidence(&request.state);
        attach_settled_group_artifacts(
            &selected,
            &request.tool_group_settlements,
            &mut context_state,
            &mut evidence,
        )?;
        append_required_recall_evidence(&selected, &mut evidence);
        let artifacts = referenced_artifacts(&snapshot, &evidence)?;
        for artifact in &artifacts {
            self.store.verify_artifact(artifact).await?;
        }

        let history_start = selected
            .first()
            .map(|group| group.history_start)
            .ok_or_else(|| {
                CheckpointError::InvalidRequest("no tool groups selected".to_string())
            })?;
        let history_end = selected
            .last()
            .and_then(|group| group.history_end)
            .ok_or_else(|| {
                CheckpointError::InvalidRequest("selected tool group is still open".to_string())
            })?;
        let record_id = TurnRecordId::new(snapshot.next_turn_record_id);
        let record = TurnRecord {
            record_id,
            generation_id: snapshot.generation_id,
            completed_groups: request.completed_tool_groups.clone(),
            tool_group_settlements: request.tool_group_settlements.clone(),
            state_removals: request.state.removals.clone(),
            state: context_state,
            summary: String::new(),
            evidence,
            changes: Vec::new(),
            validation: Vec::new(),
            decisions: Vec::new(),
            open_items: Vec::new(),
            correction_of: None,
        };
        let record_bytes = serde_json::to_vec(&record)
            .map_err(|error| CheckpointError::InvalidRequest(error.to_string()))?;
        if record_bytes.len() > MAX_CHECKPOINT_RECORD_BYTES {
            return Err(CheckpointError::InvalidRequest(format!(
                "context state checkpoint is {} bytes; maximum is {MAX_CHECKPOINT_RECORD_BYTES}",
                record_bytes.len()
            )));
        }
        self.store.write_turn_record(&record).await?;
        let pending = PendingCheckpoint {
            record: record.clone(),
            history_start,
            history_end,
            completed_groups: record.completed_groups.clone(),
            manifest_sha256: String::new(),
        };
        {
            let mut state = self.state.lock().await;
            if state.manifest.next_turn_record_id != snapshot.next_turn_record_id
                || state.manifest.pending_checkpoint.is_some()
            {
                return Err(CheckpointError::InvalidRequest(
                    "checkpoint state changed while evidence was being verified".to_string(),
                ));
            }
            state.manifest.next_turn_record_id =
                state.manifest.next_turn_record_id.saturating_add(1);
            state.manifest.turn_records.push(record);
            state.manifest.pending_checkpoint = Some(pending);
        }
        let manifest_sha256 = match self.persist().await {
            Ok(manifest_sha256) => manifest_sha256,
            Err(error) => {
                self.state.lock().await.manifest.pending_checkpoint = None;
                self.degrade(error.to_string()).await;
                return Err(CheckpointError::Storage(format!(
                    "failed to freeze checkpoint manifest: {error}"
                )));
            }
        };
        let mut pending = self
            .take_pending_checkpoint()
            .await
            .ok_or(CheckpointError::NoPendingCheckpoint)?;
        pending.manifest_sha256 = manifest_sha256;
        Ok(pending)
    }

    pub async fn take_pending_checkpoint(&self) -> Option<PendingCheckpoint> {
        let state = self.state.lock().await;
        state
            .manifest
            .pending_checkpoint
            .clone()
            .map(|mut pending| {
                pending.manifest_sha256 = state.manifest_sha256.clone().unwrap_or_default();
                pending
            })
    }

    pub async fn mark_installed(
        &self,
        installed: InstalledCheckpoint,
    ) -> Result<(), CheckpointError> {
        {
            let mut state = self.state.lock().await;
            let pending = state
                .manifest
                .pending_checkpoint
                .clone()
                .ok_or(CheckpointError::NoPendingCheckpoint)?;
            if pending.record.record_id != installed.record_id {
                return Err(CheckpointError::InvalidRequest(format!(
                    "installed record {} does not match pending record {}",
                    installed.record_id, pending.record.record_id
                )));
            }
            let completed: HashSet<_> = pending.completed_groups.iter().copied().collect();
            state
                .manifest
                .tool_groups
                .retain(|group| !completed.contains(&group.group_id));
            for group in &mut state.manifest.tool_groups {
                group.history_start = shifted_history_index(
                    group.history_start,
                    &pending,
                    installed.replacement_item_count,
                )?;
                if let Some(history_end) = &mut group.history_end {
                    *history_end = shifted_history_index(
                        *history_end,
                        &pending,
                        installed.replacement_item_count,
                    )?;
                }
            }
            if let Some(active) = &mut state.manifest.active_sampling {
                active.history_start = shifted_history_index(
                    active.history_start,
                    &pending,
                    installed.replacement_item_count,
                )?;
            }
            state.manifest.pending_checkpoint = None;
            state.manifest.last_installed = Some(installed);
            state.manifest.generation_id =
                CheckpointGenerationId::new(state.manifest.generation_id.get().saturating_add(1));
        }
        match self.persist().await {
            Ok(_) => Ok(()),
            Err(error) => {
                self.degrade(error.to_string()).await;
                Err(error)
            }
        }
    }

    pub async fn recall(&self, request: RecallRequest) -> Result<RecallResult, CheckpointError> {
        if request.max_bytes == 0 {
            return Err(CheckpointError::InvalidRequest(
                "recall maxBytes must be greater than zero".to_string(),
            ));
        }
        let artifact = {
            let state = self.state.lock().await;
            let artifact_id = match &request.locator {
                ArtifactLocator::ArtifactId(artifact_id) => artifact_id.clone(),
                ArtifactLocator::Reference(reference) => {
                    resolve_artifact_reference(&state.manifest, reference).ok_or_else(|| {
                        CheckpointError::UntrustedEvidence(format!(
                            "unknown checkpoint artifact reference {reference}"
                        ))
                    })?
                }
            };
            find_artifact(&state.manifest, &artifact_id).ok_or_else(|| {
                CheckpointError::UntrustedEvidence(format!(
                    "artifact {artifact_id} is not referenced by this session"
                ))
            })?
        };
        let data = self.store.read_verified_artifact(&artifact).await?;
        crate::artifact_recall::build_recall_result(
            artifact,
            data,
            request.selection,
            request.max_bytes.min(MAX_RECALL_BYTES),
        )
    }

    pub async fn is_fallback_required(&self) -> bool {
        let state = self.state.lock().await;
        state.degraded_reason.is_some()
    }

    async fn prepared_status(
        &self,
        usage: ContextUsageSnapshot,
        tool_result_share_basis_points: u16,
    ) -> PreparedCheckpointRequest {
        let mut state = self.state.lock().await;
        let usage_basis_points = self.budget.usage_basis_points(usage);
        let pressure = self.budget.pressure(usage_basis_points);
        let settleable_group_ids = settleable_groups(&state.manifest)
            .into_iter()
            .map(|group| group.group_id)
            .collect::<Vec<_>>();
        let open_groups = state
            .manifest
            .tool_groups
            .iter()
            .filter(|group| group.state == ToolGroupState::Open)
            .count();
        let settled_groups = state
            .manifest
            .tool_groups
            .iter()
            .filter(|group| group.state == ToolGroupState::Settled)
            .count();
        let checkpoint_only = matches!(
            pressure,
            CheckpointPressure::Required | CheckpointPressure::FallbackRequired
        ) && !settleable_group_ids.is_empty()
            && state.degraded_reason.is_none();
        let tool_policy = if checkpoint_only {
            CheckpointToolPolicy::CheckpointOnly
        } else {
            CheckpointToolPolicy::Normal
        };
        state.last_tool_policy = tool_policy;
        PreparedCheckpointRequest {
            record: state.manifest.turn_records.last().cloned(),
            status: MemoryStatusSnapshot {
                generation_id: state.manifest.generation_id,
                completed_sessions: state.manifest.generation_id.get().saturating_sub(1),
                quarter_consolidation_due: state
                    .manifest
                    .generation_id
                    .get()
                    .is_multiple_of(SESSIONS_PER_QUARTER),
                turn_id: state
                    .manifest
                    .active_sampling
                    .as_ref()
                    .map(|sampling| sampling.turn_id.clone())
                    .unwrap_or_default(),
                context_usage_basis_points: usage_basis_points,
                usage_source: usage.usage_source,
                tool_result_share_basis_points: tool_result_share_basis_points.min(10_000),
                open_groups,
                settled_groups,
                settleable_group_ids,
                last_checkpoint: state
                    .manifest
                    .last_installed
                    .as_ref()
                    .map(|installed| installed.record_id),
                pressure,
                degraded_reason: state.degraded_reason.clone(),
            },
            tool_policy,
        }
    }

    pub(crate) async fn persist(&self) -> Result<String, CheckpointError> {
        let _persist_guard = self.persist_lock.lock().await;
        let manifest = self.state.lock().await.manifest.clone();
        let sha256 = self.store.write_manifest(&manifest).await?;
        self.state.lock().await.manifest_sha256 = Some(sha256.clone());
        Ok(sha256)
    }

    async fn capture_failure(
        &self,
        call_id: String,
        error: CheckpointError,
    ) -> ArtifactCaptureOutcome {
        let message = error.to_string();
        self.degrade(message.clone()).await;
        ArtifactCaptureOutcome::Degraded(ArtifactCaptureFailure { call_id, message })
    }

    async fn degrade(&self, reason: String) {
        let mut state = self.state.lock().await;
        if state.degraded_reason.is_none() {
            state.degraded_reason = Some(reason);
        }
    }
}

fn validate_group_selection<'a>(
    manifest: &'a RuntimeManifest,
    request: &UpdateContextStateRequest,
) -> Result<Vec<&'a ToolGroupRecord>, CheckpointError> {
    if request.completed_tool_groups.is_empty() {
        return Err(CheckpointError::InvalidRequest(
            "completedToolGroups must select at least one group".to_string(),
        ));
    }
    let settleable = settleable_groups(manifest);
    if request.completed_tool_groups.len() > settleable.len() {
        return Err(CheckpointError::InvalidRequest(
            "completedToolGroups includes an open or unknown group".to_string(),
        ));
    }
    let selected = &settleable[..request.completed_tool_groups.len()];
    let actual = selected
        .iter()
        .map(|group| group.group_id)
        .collect::<Vec<_>>();
    if actual != request.completed_tool_groups {
        return Err(CheckpointError::InvalidRequest(format!(
            "completedToolGroups must be the contiguous active prefix; expected {actual:?}"
        )));
    }
    Ok(selected.to_vec())
}

fn settleable_groups(manifest: &RuntimeManifest) -> Vec<&ToolGroupRecord> {
    manifest
        .tool_groups
        .iter()
        .take_while(|group| group.state == ToolGroupState::Settled)
        .collect()
}

fn referenced_artifacts(
    manifest: &RuntimeManifest,
    evidence: &[EvidenceRef],
) -> Result<Vec<ArtifactRef>, CheckpointError> {
    evidence
        .iter()
        .filter_map(|reference| match reference {
            EvidenceRef::Artifact { artifact_id } => Some(artifact_id),
            EvidenceRef::WorkspacePath { .. } | EvidenceRef::Symbol { .. } => None,
        })
        .map(|artifact_id| {
            find_artifact(manifest, artifact_id).ok_or_else(|| {
                CheckpointError::UntrustedEvidence(format!(
                    "artifact {artifact_id} is not referenced by this session"
                ))
            })
        })
        .collect()
}

fn append_required_recall_evidence(selected: &[&ToolGroupRecord], evidence: &mut Vec<EvidenceRef>) {
    let mut evidence_ids = evidence
        .iter()
        .filter_map(|reference| match reference {
            EvidenceRef::Artifact { artifact_id } => Some(artifact_id.clone()),
            EvidenceRef::WorkspacePath { .. } | EvidenceRef::Symbol { .. } => None,
        })
        .collect::<HashSet<_>>();
    for group in selected.iter().filter(|group| group.requires_recall) {
        for call in &group.calls {
            if (call.truncation != TruncationProvenance::Complete
                || matches!(&call.outcome, ToolCallOutcome::ToolError { .. }))
                && evidence_ids.insert(call.output.artifact_id.clone())
            {
                evidence.push(EvidenceRef::Artifact {
                    artifact_id: call.output.artifact_id.clone(),
                });
            }
        }
    }
}

fn find_artifact(manifest: &RuntimeManifest, artifact_id: &ArtifactId) -> Option<ArtifactRef> {
    manifest
        .artifacts
        .iter()
        .chain(
            manifest
                .tool_groups
                .iter()
                .flat_map(|group| &group.calls)
                .flat_map(|call| [&call.input, &call.output]),
        )
        .find(|artifact| &artifact.artifact_id == artifact_id)
        .cloned()
}

fn resolve_artifact_reference(manifest: &RuntimeManifest, reference: &str) -> Option<ArtifactId> {
    let record_id = reference
        .strip_prefix("artifact:")?
        .split_once("/A")?
        .0
        .parse::<TurnRecordId>()
        .ok()?;
    let record = manifest
        .turn_records
        .iter()
        .find(|record| record.record_id == record_id)?;
    crate::model_projection::artifact_id_for_reference(record, reference).cloned()
}

fn shifted_history_index(
    index: usize,
    pending: &PendingCheckpoint,
    replacement_item_count: usize,
) -> Result<usize, CheckpointError> {
    if index <= pending.history_start {
        return Ok(index);
    }
    if index < pending.history_end {
        return Err(CheckpointError::InvalidRequest(format!(
            "open history index {index} overlaps installed checkpoint {}..{}",
            pending.history_start, pending.history_end
        )));
    }
    Ok(pending.history_start + replacement_item_count + (index - pending.history_end))
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
