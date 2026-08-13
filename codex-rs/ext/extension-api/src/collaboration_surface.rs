use crate::ExtensionData;

/// Thread-scoped policy for the host-owned generic collaboration surface.
///
/// Extensions use this to suppress generic sub-agent tools and prompt fragments
/// for a specialized thread without changing review, guardian, or unrelated
/// Codex sessions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CollaborationSurfacePolicy {
    /// Keep the host's normal generic collaboration behavior.
    #[default]
    Enabled,
    /// Hide generic collaboration tools, usage hints, and mode instructions.
    Disabled,
}

/// Contributes thread-scoped policy for generic collaboration surfaces.
pub trait CollaborationSurfaceContributor: Send + Sync {
    fn policy(
        &self,
        session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> CollaborationSurfacePolicy;
}
