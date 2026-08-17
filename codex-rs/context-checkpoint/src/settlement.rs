use crate::CheckpointError;
use crate::ContextStateSnapshot;
use crate::ToolGroupDisposition;
use crate::ToolGroupRecord;
use crate::ToolGroupSettlement;
use std::collections::HashSet;

const MAX_SETTLEMENT_LINKS: usize = 32;
const MAX_SETTLEMENT_VALUE_BYTES: usize = 1024;

pub(crate) fn validate_tool_group_settlements(
    selected: &[&ToolGroupRecord],
    settlements: &[ToolGroupSettlement],
    state: &ContextStateSnapshot,
) -> Result<(), CheckpointError> {
    if settlements.len() != selected.len() {
        return Err(CheckpointError::InvalidRequest(format!(
            "toolGroupSettlements must contain exactly one entry for each of the {} completed tool groups",
            selected.len()
        )));
    }

    let retained_keys = state
        .quarter
        .iter()
        .chain(&state.session)
        .chain(&state.active.entries)
        .map(|entry| entry.key.as_str())
        .collect::<HashSet<_>>();
    let open_questions = state
        .active
        .open_questions
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();

    for (group, settlement) in selected.iter().zip(settlements) {
        if settlement.group_id != group.group_id {
            return Err(CheckpointError::InvalidRequest(format!(
                "toolGroupSettlements must follow completedToolGroups order; expected {}, found {}",
                group.group_id, settlement.group_id
            )));
        }

        validate_unique_values("stateKeys", &settlement.state_keys)?;
        validate_unique_values("openQuestions", &settlement.open_questions)?;
        for key in &settlement.state_keys {
            if !retained_keys.contains(key.as_str()) {
                return Err(CheckpointError::InvalidRequest(format!(
                    "settlement for {} references state key {key}, but that key is not retained",
                    group.group_id
                )));
            }
        }
        for question in &settlement.open_questions {
            if !open_questions.contains(question.as_str()) {
                return Err(CheckpointError::InvalidRequest(format!(
                    "settlement for {} references an open question that is not active: {question}",
                    group.group_id
                )));
            }
        }

        match settlement.disposition {
            ToolGroupDisposition::Promote if settlement.state_keys.is_empty() => {
                return Err(CheckpointError::InvalidRequest(format!(
                    "promote settlement for {} requires at least one retained state key",
                    group.group_id
                )));
            }
            ToolGroupDisposition::KeepOpen if settlement.open_questions.is_empty() => {
                return Err(CheckpointError::InvalidRequest(format!(
                    "keepOpen settlement for {} requires at least one active open question",
                    group.group_id
                )));
            }
            ToolGroupDisposition::ArchiveOnly
                if !settlement.state_keys.is_empty() || !settlement.open_questions.is_empty() =>
            {
                return Err(CheckpointError::InvalidRequest(format!(
                    "archiveOnly settlement for {} cannot retain state keys or open questions",
                    group.group_id
                )));
            }
            ToolGroupDisposition::Promote
            | ToolGroupDisposition::KeepOpen
            | ToolGroupDisposition::ArchiveOnly => {}
        }
    }

    Ok(())
}

fn validate_unique_values(field: &str, values: &[String]) -> Result<(), CheckpointError> {
    if values.len() > MAX_SETTLEMENT_LINKS {
        return Err(CheckpointError::InvalidRequest(format!(
            "toolGroupSettlements.{field} contains {} values; maximum is {MAX_SETTLEMENT_LINKS}",
            values.len()
        )));
    }
    let mut unique = HashSet::new();
    for value in values {
        if value.trim().is_empty() {
            return Err(CheckpointError::InvalidRequest(format!(
                "toolGroupSettlements.{field} cannot contain empty values"
            )));
        }
        if !unique.insert(value.as_str()) {
            return Err(CheckpointError::InvalidRequest(format!(
                "toolGroupSettlements.{field} contains duplicate value {value}"
            )));
        }
        if value.len() > MAX_SETTLEMENT_VALUE_BYTES {
            return Err(CheckpointError::InvalidRequest(format!(
                "toolGroupSettlements.{field} value is {} bytes; maximum is {MAX_SETTLEMENT_VALUE_BYTES}",
                value.len()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "settlement_tests.rs"]
mod tests;
