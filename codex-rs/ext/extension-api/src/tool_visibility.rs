use std::collections::BTreeSet;

use codex_tools::ToolName;

use crate::ExtensionData;

/// A composable restriction over tools available to one thread.
///
/// An absent allow set leaves the current tool set unchanged. When multiple
/// policies provide allow sets, only names present in every allow set remain.
/// Deny sets are always combined by union and take precedence over allows.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolVisibilityPolicy {
    pub allow: Option<BTreeSet<ToolName>>,
    pub deny: BTreeSet<ToolName>,
}

impl ToolVisibilityPolicy {
    /// Creates a policy that exposes only the supplied tool names.
    pub fn allow_only(names: impl IntoIterator<Item = ToolName>) -> Self {
        Self {
            allow: Some(names.into_iter().collect()),
            deny: BTreeSet::new(),
        }
    }

    /// Returns whether a concrete tool name survives this policy.
    pub fn allows(&self, name: &ToolName) -> bool {
        self.allow
            .as_ref()
            .is_none_or(|allowed| allowed.contains(name))
            && !self.deny.contains(name)
    }

    /// Combines another contributor policy into this policy.
    pub fn intersect(&mut self, other: Self) {
        self.allow = match (self.allow.take(), other.allow) {
            (Some(current), Some(next)) => Some(current.intersection(&next).cloned().collect()),
            (Some(current), None) => Some(current),
            (None, Some(next)) => Some(next),
            (None, None) => None,
        };
        self.deny.extend(other.deny);
    }
}

/// Contributes thread-scoped tool restrictions before the host constructs its
/// model-visible specifications and executable registry.
///
/// Implementations should return immutable policy derived from extension data.
/// They must not assume that hiding a model specification alone prevents direct
/// dispatch; hosts apply the resulting policy to both specifications and
/// executors.
pub trait ToolVisibilityContributor: Send + Sync {
    fn visibility(
        &self,
        session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> ToolVisibilityPolicy;
}
