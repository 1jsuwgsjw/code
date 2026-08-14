use crate::CheckpointError;
use crate::PendingCheckpoint;

pub fn project_history<T: Clone>(
    history: &[T],
    pending: &PendingCheckpoint,
    checkpoint_item: T,
) -> Result<Vec<T>, CheckpointError> {
    if pending.history_start >= pending.history_end || pending.history_end > history.len() {
        return Err(CheckpointError::InvalidRequest(format!(
            "checkpoint range {}..{} is invalid for history length {}",
            pending.history_start,
            pending.history_end,
            history.len()
        )));
    }
    let mut projected =
        Vec::with_capacity(history.len() - (pending.history_end - pending.history_start) + 1);
    projected.extend_from_slice(&history[..pending.history_start]);
    projected.push(checkpoint_item);
    projected.extend_from_slice(&history[pending.history_end..]);
    Ok(projected)
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
