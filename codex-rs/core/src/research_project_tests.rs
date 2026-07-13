use super::*;
use pretty_assertions::assert_eq;

#[test]
fn remote_identity_ignores_root_commits_but_keeps_path_bridge() {
    let aliases = aliases_from_evidence(
        Path::new("/workspace/project"),
        BTreeSet::from(["github.com/openai/codex".to_string()]),
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
        BTreeSet::new(),
        vec!["b".to_string(), "a".to_string(), "a".to_string()],
    );

    assert_eq!(aliases.len(), 2);
    assert!(aliases.iter().any(|alias| alias.starts_with("git_roots:")));
    assert!(aliases.iter().any(|alias| alias.starts_with("path:")));
}
