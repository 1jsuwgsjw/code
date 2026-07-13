use super::*;
use pretty_assertions::assert_eq;

#[test]
fn remote_identity_ignores_root_commits_but_keeps_path_bridge() {
    let aliases = aliases_from_evidence(
        Path::new("/workspace/project"),
        Some("github.com/openai/codex".to_string()),
        vec!["root-commit".to_string()],
    );

    assert_eq!(aliases.len(), 2);
    assert!(aliases.iter().any(|alias| alias.starts_with("git_remote:")));
    assert!(aliases.iter().any(|alias| alias.starts_with("path:")));
    assert!(!aliases.iter().any(|alias| alias.starts_with("git_roots:")));
}

#[test]
fn repository_roots_are_used_when_remote_is_missing() {
    let aliases = aliases_from_evidence(
        Path::new("/workspace/project"),
        None,
        vec!["b".to_string(), "a".to_string(), "a".to_string()],
    );

    assert_eq!(aliases.len(), 2);
    assert!(aliases.iter().any(|alias| alias.starts_with("git_roots:")));
    assert!(aliases.iter().any(|alias| alias.starts_with("path:")));
}

#[test]
fn origin_is_preferred_over_shared_upstream() {
    let remote = preferred_remote(BTreeMap::from([
        (
            "origin".to_string(),
            "https://github.com/example/fork.git".to_string(),
        ),
        (
            "upstream".to_string(),
            "https://github.com/openai/codex.git".to_string(),
        ),
    ]));

    assert_eq!(remote, Some("github.com/example/fork".to_string()));
}
