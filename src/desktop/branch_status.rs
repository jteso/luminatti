//! Local tracking information; never fetches or assumes an origin/main upstream.
use std::{collections::HashMap, path::Path};

use git2::{BranchType, Repository};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum BranchStatus {
    Tracking {
        upstream: String,
        ahead: usize,
        behind: usize,
    },
    NoUpstream,
    Unavailable,
}

impl BranchStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Tracking {
                ahead: 0,
                behind: 0,
                ..
            } => "✓".into(),
            Self::Tracking { ahead, behind, .. } => [
                (*ahead > 0).then(|| format!("↑{ahead}")),
                (*behind > 0).then(|| format!("↓{behind}")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" "),
            Self::NoUpstream => "—".into(),
            Self::Unavailable => "?".into(),
        }
    }

    pub fn tooltip(&self) -> String {
        match self {
            Self::Tracking { upstream, ahead, behind } => format!(
                "{ahead} commits ahead · {behind} commits behind {upstream}\nCompared with the last fetched remote state."
            ),
            Self::NoUpstream => "No remote upstream configured for this branch.".into(),
            Self::Unavailable => "Remote tracking status unavailable. The upstream may have been deleted or not fetched yet.".into(),
        }
    }
}

pub(super) fn load(root: &Path) -> HashMap<String, BranchStatus> {
    let Ok(repo) = Repository::discover(root) else {
        return HashMap::new();
    };
    let Ok(branches) = repo.branches(Some(BranchType::Local)) else {
        return HashMap::new();
    };
    branches
        .filter_map(Result::ok)
        .filter_map(|(branch, _)| {
            let name = branch.name().ok().flatten()?.to_string();
            let status = (|| {
                let config = repo.config().ok()?;
                let remote = config.get_string(&format!("branch.{name}.remote")).ok();
                let merge = config.get_string(&format!("branch.{name}.merge")).ok();
                if remote.is_none() || merge.is_none() || remote.as_deref() == Some(".") {
                    return Some(BranchStatus::NoUpstream);
                }
                let upstream = branch.upstream().ok()?;
                let upstream_name = upstream.name().ok().flatten()?.to_string();
                let (ahead, behind) = repo
                    .graph_ahead_behind(
                        branch.get().peel_to_commit().ok()?.id(),
                        upstream.get().peel_to_commit().ok()?.id(),
                    )
                    .ok()?;
                Some(BranchStatus::Tracking {
                    upstream: upstream_name,
                    ahead,
                    behind,
                })
            })()
            .unwrap_or(BranchStatus::Unavailable);
            Some((name, status))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(repo: &Repository, parent: Option<git2::Oid>, message: &str) -> git2::Oid {
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Test", "test@example.com").unwrap();
        let parents: Vec<_> = parent
            .map(|id| repo.find_commit(id).unwrap())
            .into_iter()
            .collect();
        repo.commit(
            None,
            &signature,
            &signature,
            message,
            &tree,
            &parents.iter().collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn tracks_configured_remote_and_counts_both_sides_of_divergence() {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        repo.remote("upstream", "https://example.com/repository.git")
            .unwrap();
        let base = commit(&repo, None, "base");
        let local = commit(&repo, Some(base), "local");
        let remote = commit(&repo, Some(base), "remote");
        let remote_tip = commit(&repo, Some(remote), "remote second");
        repo.reference("refs/heads/topic", local, true, "test")
            .unwrap();
        repo.reference(
            "refs/remotes/upstream/different-name",
            remote_tip,
            true,
            "test",
        )
        .unwrap();
        repo.find_branch("topic", BranchType::Local)
            .unwrap()
            .set_upstream(Some("upstream/different-name"))
            .unwrap();

        for (local_tip, remote_tip, ahead, behind, label) in [
            (local, remote_tip, 1, 2, "↑1 ↓2"),
            (local, base, 1, 0, "↑1"),
            (base, remote_tip, 0, 2, "↓2"),
            (base, base, 0, 0, "✓"),
        ] {
            repo.reference("refs/heads/topic", local_tip, true, "test")
                .unwrap();
            repo.reference(
                "refs/remotes/upstream/different-name",
                remote_tip,
                true,
                "test",
            )
            .unwrap();
            let statuses = load(temp.path());
            let status = &statuses["topic"];
            assert_eq!(
                status,
                &BranchStatus::Tracking {
                    upstream: "upstream/different-name".into(),
                    ahead,
                    behind,
                }
            );
            assert_eq!(status.label(), label);
        }
        // Reading local branches remains safe when HEAD itself is detached.
        repo.set_head_detached(local).unwrap();
        assert_eq!(load(temp.path())["topic"].label(), "✓");
        repo.find_reference("refs/remotes/upstream/different-name")
            .unwrap()
            .delete()
            .unwrap();
        assert_eq!(load(temp.path())["topic"], BranchStatus::Unavailable);
        repo.find_branch("topic", BranchType::Local)
            .unwrap()
            .set_upstream(None)
            .unwrap();
        assert_eq!(load(temp.path())["topic"], BranchStatus::NoUpstream);
    }

    #[test]
    fn reads_shared_remote_refs_from_linked_worktree() {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path().join("main")).unwrap();
        let base = commit(&repo, None, "base");
        let local = commit(&repo, Some(base), "local");
        repo.reference("refs/heads/main", local, true, "test")
            .unwrap();
        repo.set_head("refs/heads/main").unwrap();
        repo.remote("origin", "https://example.com/repository.git")
            .unwrap();
        repo.reference("refs/remotes/origin/main", base, true, "test")
            .unwrap();
        repo.find_branch("main", BranchType::Local)
            .unwrap()
            .set_upstream(Some("origin/main"))
            .unwrap();
        let worktree = temp.path().join("linked");
        repo.worktree("linked", &worktree, None).unwrap();
        assert_eq!(load(&worktree)["main"].label(), "↑1");
    }

    #[test]
    fn empty_or_non_repository_has_no_status() {
        let temp = tempfile::tempdir().unwrap();
        assert!(load(temp.path()).is_empty());
        Repository::init(temp.path()).unwrap();
        assert!(load(temp.path()).is_empty());
    }
}
