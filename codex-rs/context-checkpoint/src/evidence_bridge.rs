use crate::CheckpointError;
use crate::ContextStateSnapshot;
use crate::EvidenceRef;
use crate::ToolGroupDisposition;
use crate::ToolGroupRecord;
use crate::ToolGroupSettlement;
use std::collections::HashSet;

const MAX_AUTOMATIC_ARTIFACTS_PER_STATE_ENTRY: usize = 3;

pub(crate) fn attach_settled_group_artifacts(
    selected: &[&ToolGroupRecord],
    settlements: &[ToolGroupSettlement],
    state: &mut ContextStateSnapshot,
    evidence: &mut Vec<EvidenceRef>,
) -> Result<(), CheckpointError> {
    let mut recorded_artifact_ids = evidence
        .iter()
        .filter_map(|reference| match reference {
            EvidenceRef::Artifact { artifact_id } => Some(artifact_id.clone()),
            EvidenceRef::WorkspacePath { .. } | EvidenceRef::Symbol { .. } => None,
        })
        .collect::<HashSet<_>>();

    for (group, settlement) in selected.iter().zip(settlements).rev() {
        if !matches!(
            settlement.disposition,
            ToolGroupDisposition::Promote | ToolGroupDisposition::KeepOpen
        ) {
            continue;
        }

        for state_key in &settlement.state_keys {
            let entry = state
                .quarter
                .iter_mut()
                .chain(state.session.iter_mut())
                .chain(state.active.entries.iter_mut())
                .find(|entry| entry.key == state_key.as_str())
                .ok_or_else(|| {
                    CheckpointError::InvalidRequest(format!(
                        "settlement state key {state_key} is not retained"
                    ))
                })?;
            let mut attached_artifact_ids = entry
                .evidence
                .iter()
                .filter_map(|reference| match reference {
                    EvidenceRef::Artifact { artifact_id } => Some(artifact_id.clone()),
                    EvidenceRef::WorkspacePath { .. } | EvidenceRef::Symbol { .. } => None,
                })
                .collect::<HashSet<_>>();

            for call in group.calls.iter().rev() {
                if attached_artifact_ids.len() >= MAX_AUTOMATIC_ARTIFACTS_PER_STATE_ENTRY {
                    break;
                }
                let artifact_id = call.output.artifact_id.clone();
                if !attached_artifact_ids.insert(artifact_id.clone()) {
                    continue;
                }

                let reference = EvidenceRef::Artifact {
                    artifact_id: artifact_id.clone(),
                };
                entry.evidence.push(reference.clone());
                if recorded_artifact_ids.insert(artifact_id) {
                    evidence.push(reference);
                }
            }
        }
    }

    Ok(())
}
