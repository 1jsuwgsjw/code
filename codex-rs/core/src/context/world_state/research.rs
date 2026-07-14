// CTF|认证 - Research context WorldState section.

use super::PreviousSectionState;
use super::WorldStateSection;
use crate::context::ContextualUserFragment;
use crate::context::ResearchContextFragment;
use codex_research_state::ResearchContextProjection;
use codex_utils_cache::sha1_digest;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, Default)]
pub(crate) struct ResearchContextState {
    projection: Option<ResearchContextProjection>,
    digest: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ResearchContextSnapshot {
    digest: Option<String>,
}

impl ResearchContextState {
    pub(crate) fn new(projection: ResearchContextProjection) -> Self {
        if projection.entries.is_empty() {
            return Self::default();
        }
        let projected_content = ResearchContextFragment::projected_content(&projection);
        let digest = Some(
            sha1_digest(projected_content.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        );
        Self {
            projection: Some(projection),
            digest,
        }
    }
}

impl WorldStateSection for ResearchContextState {
    const ID: &'static str = "research_context";
    type Snapshot = ResearchContextSnapshot;

    fn snapshot(&self) -> Self::Snapshot {
        ResearchContextSnapshot {
            digest: self.digest.clone(),
        }
    }

    fn matches_legacy_fragment(role: &str, text: &str) -> bool {
        role == "user" && ResearchContextFragment::matches_text(text)
    }

    fn has_retained_fragment_matcher(&self) -> bool {
        true
    }

    fn matches_retained_fragment(&self, role: &str, text: &str) -> bool {
        if role != "user" || !ResearchContextFragment::matches_text(text) {
            return false;
        }
        self.projection.as_ref().is_none_or(|projection| {
            text.contains(ResearchContextFragment::projected_content(projection).as_str())
        })
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        let current = self.snapshot();
        if matches!(previous, PreviousSectionState::Known(previous) if previous == &current) {
            return None;
        }
        let previous_had_projection = match previous {
            PreviousSectionState::Known(previous) => previous.digest.is_some(),
            PreviousSectionState::Unknown => true,
            PreviousSectionState::Absent => false,
        };
        match self.projection.as_ref() {
            Some(projection) => Some(Box::new(ResearchContextFragment::projected(
                projection,
                previous_had_projection,
            ))),
            None if previous_had_projection => Some(Box::new(ResearchContextFragment::removed())),
            None => None,
        }
    }
}

#[cfg(test)]
#[path = "research_tests.rs"]
mod tests;
