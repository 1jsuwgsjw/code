use codex_git_utils::canonicalize_git_remote_url;
use codex_git_utils::get_git_remote_urls_assume_git_repo;
use codex_git_utils::get_git_repo_root;
use codex_git_utils::get_git_root_commit_hashes;
use codex_utils_path::normalize_for_path_comparison;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use uuid::Uuid;

const RESEARCH_ALIAS_NAMESPACE: Uuid = Uuid::from_bytes([
    0x0b, 0xcb, 0x2b, 0x8e, 0x42, 0xfa, 0x44, 0x2f, 0xa1, 0x5d, 0x4b, 0xb6, 0xf6, 0xb0, 0xcb, 0x6c,
]);

pub(crate) async fn discover_research_project_aliases(cwd: &Path) -> Vec<String> {
    let root = get_git_repo_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let remotes = get_git_remote_urls_assume_git_repo(root.as_path())
        .await
        .unwrap_or_default();
    let remote = preferred_remote(remotes);
    let root_commits = if remote.is_none() {
        get_git_root_commit_hashes(root.as_path())
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|hash| hash.0)
            .collect()
    } else {
        Vec::new()
    };
    aliases_from_evidence(root.as_path(), remote, root_commits)
}

fn aliases_from_evidence(
    root: &Path,
    remote: Option<String>,
    mut root_commits: Vec<String>,
) -> Vec<String> {
    let mut aliases = BTreeSet::new();
    if let Some(remote) = remote {
        aliases.insert(opaque_alias("git_remote", remote.as_str()));
    } else {
        root_commits.sort_unstable();
        root_commits.dedup();
        if !root_commits.is_empty() {
            aliases.insert(opaque_alias("git_roots", root_commits.join("\n").as_str()));
        }
    }
    aliases.insert(opaque_alias(
        "path",
        normalized_path_material(root).as_str(),
    ));
    aliases.into_iter().collect()
}

fn preferred_remote(remotes: BTreeMap<String, String>) -> Option<String> {
    if let Some(origin) = remotes
        .get("origin")
        .and_then(|remote| canonicalize_git_remote_url(remote))
    {
        return Some(origin);
    }
    remotes
        .into_iter()
        .find_map(|(_, remote)| canonicalize_git_remote_url(remote.as_str()))
}

fn opaque_alias(kind: &str, material: &str) -> String {
    let name = format!("{kind}\0{material}");
    format!(
        "{kind}:{}",
        Uuid::new_v5(&RESEARCH_ALIAS_NAMESPACE, name.as_bytes())
    )
}

fn normalized_path_material(path: &Path) -> String {
    let normalized = normalize_for_path_comparison(path).unwrap_or_else(|_| path.to_path_buf());
    let mut material = normalized.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        material.make_ascii_lowercase();
    }
    material
}

#[cfg(test)]
#[path = "research_project_tests.rs"]
mod tests;
