use super::*;

fn checkpoint() -> ContextCheckpointDetails {
    ContextCheckpointDetails {
        checkpoint_ref: "ctx:G000002/TR000018".to_string(),
        generation_id: "G000002".to_string(),
        turn_record_id: "TR000018".to_string(),
        labels: vec!["decision".to_string(), "source".to_string()],
        state_entry_count: 4,
        window_number: 3,
        first_window_id: "01911111-1111-7111-8111-111111111111".to_string(),
        previous_window_id: Some("01922222-2222-7222-8222-222222222222".to_string()),
        window_id: "01933333-3333-7333-8333-333333333333".to_string(),
    }
}

fn rendered(lines: Vec<Line<'static>>) -> String {
    lines
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn checkpoint_history_tree_expands_and_folds_by_width() {
    let cell = ContextCheckpointCell::new(Some(checkpoint()));

    insta::assert_snapshot!(
        rendered(cell.render_lines(100, TreeStyle::Unicode)),
        @r###"
    • Context checkpoint ctx:G000002/TR000018
      ├─ state 4 · decision, source
      ├─ generation G000002 · record TR000018
      └─ window 3 · 01922222 -> 01933333
    "###
    );
    insta::assert_snapshot!(
        rendered(cell.render_lines(50, TreeStyle::Ascii)),
        @r###"
    * Context checkpoint ctx:G000002/TR000018
      4 state entries · window 3
    "###
    );
}
