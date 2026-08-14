use crate::models::ResponseItem;
use crate::protocol::CompactedItem;
use serde::Deserialize;

// Before `window_number` was introduced, the numeric window number was serialized as
// `window_id`. Accept that shape so existing rollouts remain resumable.
impl<'de> Deserialize<'de> for CompactedItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let serialized = SerializedCompactedItem::deserialize(deserializer)?;
        let mut window_number = serialized.window_number;
        let window_id = match serialized.window_id {
            Some(SerializedWindowId::Id(window_id)) => Some(window_id),
            Some(SerializedWindowId::LegacyWindowNumber(legacy_window_number)) => {
                window_number.get_or_insert(legacy_window_number);
                None
            }
            None => None,
        };
        Ok(Self {
            message: serialized.message,
            replacement_history: serialized.replacement_history,
            checkpoint: serialized.checkpoint,
            window_number,
            first_window_id: serialized.first_window_id,
            previous_window_id: serialized.previous_window_id,
            window_id,
        })
    }
}

#[derive(Deserialize)]
struct SerializedCompactedItem {
    message: String,
    #[serde(default)]
    replacement_history: Option<Vec<ResponseItem>>,
    #[serde(default)]
    checkpoint: Option<crate::protocol::ContextCheckpointRolloutMetadata>,
    #[serde(default)]
    window_number: Option<u64>,
    #[serde(default)]
    first_window_id: Option<String>,
    #[serde(default)]
    previous_window_id: Option<String>,
    #[serde(default)]
    window_id: Option<SerializedWindowId>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SerializedWindowId {
    Id(String),
    LegacyWindowNumber(u64),
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn serializes_window_number_and_id() -> Result<()> {
        let item = CompactedItem {
            message: "summary".to_string(),
            replacement_history: None,
            checkpoint: None,
            window_number: Some(3),
            first_window_id: Some("019b3f6e-0000-7000-8000-000000000001".to_string()),
            previous_window_id: Some("019b3f6e-0000-7000-8000-000000000002".to_string()),
            window_id: Some("019b3f6e-7a10-7cc3-8b6e-1d09e2f7a001".to_string()),
        };

        assert_eq!(
            serde_json::to_value(item)?,
            json!({
                "message": "summary",
                "window_number": 3,
                "first_window_id": "019b3f6e-0000-7000-8000-000000000001",
                "previous_window_id": "019b3f6e-0000-7000-8000-000000000002",
                "window_id": "019b3f6e-7a10-7cc3-8b6e-1d09e2f7a001",
            })
        );
        Ok(())
    }

    #[test]
    fn round_trips_checkpoint_metadata() -> Result<()> {
        let item = CompactedItem {
            message: "checkpoint summary".to_string(),
            replacement_history: None,
            checkpoint: Some(crate::protocol::ContextCheckpointRolloutMetadata {
                checkpoint_id: "checkpoint-TR000007".to_string(),
                generation_id: "G000003".to_string(),
                turn_record_id: "TR000007".to_string(),
                manifest_sha256: "abc123".to_string(),
                fallback_kind: Some(crate::protocol::ContextFallbackKind::RemoteProvider),
            }),
            window_number: Some(4),
            first_window_id: None,
            previous_window_id: None,
            window_id: None,
        };

        let serialized = serde_json::to_value(&item)?;
        assert_eq!(
            serialized["checkpoint"],
            serde_json::json!({
                "checkpointId": "checkpoint-TR000007",
                "generationId": "G000003",
                "turnRecordId": "TR000007",
                "manifestSha256": "abc123",
                "fallbackKind": "remoteProvider",
            })
        );
        assert_eq!(serde_json::from_value::<CompactedItem>(serialized)?, item);

        Ok(())
    }

    #[test]
    fn migrates_legacy_numeric_window_id() -> Result<()> {
        let item = serde_json::from_value::<CompactedItem>(json!({
            "message": "summary",
            "window_id": 3,
        }))?;

        assert_eq!(
            item,
            CompactedItem {
                message: "summary".to_string(),
                replacement_history: None,
                checkpoint: None,
                window_number: Some(3),
                first_window_id: None,
                previous_window_id: None,
                window_id: None,
            }
        );
        Ok(())
    }
}
