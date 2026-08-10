use super::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectAgentMaintenanceError {
    #[error(transparent)]
    Store(#[from] ProjectAgentStoreError),
    #[error(transparent)]
    InvalidDefinition(#[from] crate::ProjectAgentValidationError),
    #[error(transparent)]
    InvalidPath(#[from] codex_utils_path_uri::PathUriParseError),
    #[error("maintenance actor must contain 1 to 256 bytes")]
    InvalidActor,
    #[error("project AGENT `{agent_id}` maintenance is already locked at `{path}`")]
    Locked {
        agent_id: ProjectAgentId,
        path: PathUri,
    },
    #[error("failed to {operation} `{path}`: {source}")]
    FileSystem {
        operation: &'static str,
        path: PathUri,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse project AGENT JSON `{path}`: {source}")]
    ParseJson {
        path: PathUri,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to parse project AGENT memory index `{path}`: {source}")]
    ParseMemoryIndex {
        path: PathUri,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to serialize project AGENT memory index `{path}`: {source}")]
    SerializeMemoryIndex {
        path: PathUri,
        #[source]
        source: toml::ser::Error,
    },
    #[error("invalid pending project AGENT item `{path}`: {reason}")]
    InvalidPendingItem { path: PathUri, reason: String },
    #[error("project AGENT directory `{path}` has more than {maximum} entries")]
    TooManyDirectoryEntries { path: PathUri, maximum: usize },
    #[error("project AGENT `{agent_id}` has more than {maximum} pending {kind:?} items")]
    TooManyPendingItems {
        agent_id: ProjectAgentId,
        kind: ProjectAgentMaintenanceItemKind,
        maximum: usize,
    },
    #[error("project AGENT file `{path}` is {actual} bytes; maximum is {maximum}")]
    FileTooLarge {
        path: PathUri,
        actual: u64,
        maximum: usize,
    },
    #[error("project AGENT accepted memory `{path}` is missing")]
    AcceptedMemoryMissing { path: PathUri },
    #[error("project AGENT accepted path `{0}` already contains different data")]
    AcceptedPathConflict(PathUri),
    #[error("project AGENT maintenance artifact `{0}` already exists")]
    ArtifactAlreadyExists(PathUri),
    #[error("serialized maintenance artifact `{path}` is {actual} bytes; maximum is {maximum}")]
    SerializedArtifactTooLarge {
        path: PathUri,
        actual: usize,
        maximum: usize,
    },
    #[error("failed to serialize project AGENT maintenance JSON: {0}")]
    SerializeJson(#[from] serde_json::Error),
    #[error("system clock is before the Unix epoch: {0}")]
    SystemTime(std::time::SystemTimeError),
    #[error("system timestamp does not fit in i64 milliseconds")]
    TimestampOverflow,
}
