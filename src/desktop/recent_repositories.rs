use git2::Repository;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_RECENT_REPOSITORIES: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentRepositories {
    repositories: Vec<PathBuf>,
}

impl RecentRepositories {
    pub fn load() -> Self {
        let Some(path) = storage_path() else {
            return Self::default();
        };
        fs::read_to_string(path)
            .ok()
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default()
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.repositories
    }

    pub fn record(&mut self, repository_root: &Path) -> io::Result<()> {
        self.insert(repository_root)?;
        self.save()
    }

    fn insert(&mut self, repository_root: &Path) -> io::Result<()> {
        let repository_root = repository_root.canonicalize()?;
        self.repositories.retain(|path| path != &repository_root);
        self.repositories.insert(0, repository_root);
        self.repositories.truncate(MAX_RECENT_REPOSITORIES);
        Ok(())
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.clear_entries();
        self.save()
    }

    fn clear_entries(&mut self) {
        self.repositories.clear();
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = storage_path() else {
            return Ok(());
        };
        let Some(directory) = path.parent() else {
            return Ok(());
        };
        fs::create_dir_all(directory)?;
        let temporary_path = path.with_extension("tmp");
        fs::write(
            &temporary_path,
            serde_json::to_vec_pretty(self).map_err(io::Error::other)?,
        )?;
        fs::rename(temporary_path, path)
    }
}

pub fn resolve_repository_root(path: &Path) -> io::Result<PathBuf> {
    let repository = Repository::discover(path).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Luminatti can only open folders inside a Git repository.",
        )
    })?;
    repository
        .workdir()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Luminatti can only open non-bare Git repositories.",
            )
        })?
        .canonicalize()
}

fn storage_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|directory| directory.join("luminatti").join("recent-repositories.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repository(path: &Path) {
        Repository::init(path).unwrap();
    }

    #[test]
    fn resolves_a_nested_folder_to_its_repository_root() {
        let temporary = TempDir::new().unwrap();
        let root = temporary.path().join("repository");
        fs::create_dir_all(root.join("nested/folder")).unwrap();
        repository(&root);

        assert_eq!(
            resolve_repository_root(&root.join("nested/folder")).unwrap(),
            root.canonicalize().unwrap()
        );
    }

    #[test]
    fn rejects_a_non_repository_folder() {
        let temporary = TempDir::new().unwrap();
        assert_eq!(
            resolve_repository_root(temporary.path())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn keeps_unique_repositories_newest_first_and_limited_to_eight() {
        let temporary = TempDir::new().unwrap();
        let mut recents = RecentRepositories::default();
        let roots = (0..9)
            .map(|index| {
                let root = temporary.path().join(format!("repository-{index}"));
                fs::create_dir_all(&root).unwrap();
                root
            })
            .collect::<Vec<_>>();

        for root in &roots {
            recents.insert(root).unwrap();
        }
        recents.insert(&roots[3]).unwrap();
        let repeated = roots[3].canonicalize().unwrap();

        assert_eq!(recents.paths().len(), MAX_RECENT_REPOSITORIES);
        assert_eq!(recents.paths()[0], repeated);
        assert!(!recents.paths().contains(&roots[0].canonicalize().unwrap()));

        recents.clear_entries();
        assert!(recents.paths().is_empty());
    }
}
