// CTF|认证 - Turn task signal tests.

use super::*;

#[test]
fn task_context_is_stable_and_bounded() {
    let input = vec![TurnInput::UserInput {
        content: vec![UserInput::Text {
            text: format!("adjust   UI layout {}", "x".repeat(8_000)),
            text_elements: Vec::new(),
        }],
        client_id: None,
    }];

    let first = ResearchTaskContext::from_turn_input(&input).expect("task context");
    let second = ResearchTaskContext::from_turn_input(&input).expect("task context");

    assert_eq!(first, second);
    assert!(first.text.len() <= MAX_RESEARCH_TASK_BYTES);
    assert!(!first.text.contains("  "));
}
