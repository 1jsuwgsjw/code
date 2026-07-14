// CTF|认证 - Turn-scoped research relevance signal.

use super::TurnInput;
use super::session::Session;
use codex_protocol::user_input::UserInput;
use codex_utils_cache::sha1_digest;

const MAX_RESEARCH_TASK_BYTES: usize = 4_096;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResearchTaskContext {
    pub(crate) signature: String,
    pub(crate) text: String,
}

impl ResearchTaskContext {
    fn from_turn_input(input: &[TurnInput]) -> Option<Self> {
        let text = input
            .iter()
            .filter_map(|item| match item {
                TurnInput::UserInput { content, .. } => Some(content.as_slice()),
                TurnInput::ResponseItem(_) | TurnInput::InterAgentCommunication(_) => None,
            })
            .flatten()
            .filter_map(|item| match item {
                UserInput::Text { text, .. } => Some(text.as_str()),
                UserInput::Skill { name, .. } | UserInput::Mention { name, .. } => {
                    Some(name.as_str())
                }
                UserInput::Image { .. } | UserInput::LocalImage { .. } => None,
                _ => None,
            })
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ");
        let text = truncate_utf8(text.as_str(), MAX_RESEARCH_TASK_BYTES);
        if text.is_empty() {
            return None;
        }
        let digest = sha1_digest(text.as_bytes());
        let signature = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Some(Self { signature, text })
    }
}

impl Session {
    pub(super) async fn update_research_task_context(&self, input: &[TurnInput]) {
        let Some(task_context) = ResearchTaskContext::from_turn_input(input) else {
            return;
        };
        self.state.lock().await.research_task_context = task_context;
    }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let end = text
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max_bytes)
        .last()
        .unwrap_or(0);
    text[..end].to_string()
}

#[cfg(test)]
#[path = "research_context_tests.rs"]
mod tests;
