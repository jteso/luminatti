//! Local, per-repository persistence for the native review workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{layout::UnchangedSection, view::ReviewChangeFamily, DiffView, FileView};

const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct StoredTab {
    pub path: String,
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct WorkspaceState {
    pub sidebar_width: Option<u32>,
    pub filtered_panel_height: Option<u32>,
    pub annotation_width: Option<u32>,
    pub outline_width: Option<u32>,
    pub diff_left_width: Option<u32>,
    pub diff_view: DiffView,
    pub show_sidebar: bool,
    pub show_annotations: bool,
    pub show_outline: bool,
    pub file_view: FileView,
    pub show_only_changes: bool,
    pub hide_deleted_entries: bool,
    pub show_file_filter: bool,
    pub collapsed_directories: BTreeSet<String>,
    pub change_type_filters: BTreeSet<ReviewChangeFamily>,
    pub file_filters: Vec<String>,
    pub hide_unchanged_sections: bool,
    pub collapsed_unchanged_sections: Vec<UnchangedSection>,
    pub tabs: Vec<StoredTab>,
    pub selected_file: Option<String>,
    pub selected_change: usize,
    pub reviewed_files: BTreeSet<String>,
    pub radar_open: bool,
    pub radar_active: bool,
    pub radar_expand_all: bool,
    pub radar_expanded: BTreeSet<String>,
    pub radar_cycles: BTreeSet<String>,
    pub radar_expanded_files: BTreeSet<String>,
    pub radar_focused_files: BTreeSet<String>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            sidebar_width: None,
            filtered_panel_height: Some(180),
            annotation_width: None,
            outline_width: None,
            diff_left_width: None,
            diff_view: DiffView::Split,
            show_sidebar: true,
            show_annotations: false,
            show_outline: false,
            file_view: FileView::CompactTree,
            show_only_changes: true,
            hide_deleted_entries: false,
            show_file_filter: false,
            collapsed_directories: BTreeSet::new(),
            change_type_filters: BTreeSet::new(),
            file_filters: Vec::new(),
            hide_unchanged_sections: false,
            collapsed_unchanged_sections: Vec::new(),
            tabs: Vec::new(),
            selected_file: None,
            selected_change: 0,
            reviewed_files: BTreeSet::new(),
            radar_open: false,
            radar_active: false,
            radar_expand_all: false,
            radar_expanded: BTreeSet::new(),
            radar_cycles: BTreeSet::new(),
            radar_expanded_files: BTreeSet::new(),
            radar_focused_files: BTreeSet::new(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct StoreFile {
    version: u32,
    workspaces: BTreeMap<String, WorkspaceState>,
}

pub(super) struct ProjectSettings {
    path: Option<PathBuf>,
    store: StoreFile,
}

impl ProjectSettings {
    pub fn load() -> Self {
        Self::load_at(storage_path())
    }

    fn load_at(path: Option<PathBuf>) -> Self {
        let store = path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str::<StoreFile>(&contents).ok())
            .filter(|store| store.version == FORMAT_VERSION)
            .unwrap_or_else(|| StoreFile {
                version: FORMAT_VERSION,
                ..StoreFile::default()
            });
        Self { path, store }
    }

    #[cfg(test)]
    fn at_path(path: PathBuf) -> Self {
        Self::load_at(Some(path))
    }

    pub fn workspace(&self, root: &Path) -> WorkspaceState {
        self.store
            .workspaces
            .get(&repository_key(root))
            .cloned()
            .unwrap_or_default()
    }

    pub fn save(&mut self, root: &Path, state: WorkspaceState) -> io::Result<()> {
        self.store.workspaces.insert(repository_key(root), state);
        let Some(path) = &self.path else {
            return Ok(());
        };
        let Some(directory) = path.parent() else {
            return Ok(());
        };
        fs::create_dir_all(directory)?;
        let temporary_path = path.with_extension("tmp");
        fs::write(
            &temporary_path,
            serde_json::to_vec_pretty(&self.store).map_err(io::Error::other)?,
        )?;
        fs::rename(temporary_path, path)
    }
}

fn storage_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|directory| directory.join("luminatti").join("project-workspaces.json"))
}

fn repository_key(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn saves_each_repository_under_an_independent_key() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("settings.json");
        let first = temporary.path().join("first");
        let second = temporary.path().join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let mut settings = ProjectSettings::at_path(path.clone());
        let mut first_state = WorkspaceState::default();
        first_state.show_sidebar = false;
        let mut second_state = WorkspaceState::default();
        second_state.show_only_changes = false;

        settings.save(&first, first_state).unwrap();
        settings.save(&second, second_state).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let store: StoreFile = serde_json::from_str(&contents).unwrap();
        assert_eq!(store.version, FORMAT_VERSION);
        assert!(!store.workspaces[&repository_key(&first)].show_sidebar);
        assert!(!store.workspaces[&repository_key(&second)].show_only_changes);
        assert!(!path.with_extension("tmp").exists());
        assert!(!first.join(".luminatti").exists());
    }

    #[test]
    fn malformed_and_unsupported_storage_falls_back_to_defaults() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("settings.json");
        fs::write(&path, "not json").unwrap();
        let settings = ProjectSettings::load_at(Some(path.clone()));
        assert_eq!(
            settings.workspace(temporary.path()),
            WorkspaceState::default()
        );

        fs::write(&path, r#"{"version":99,"workspaces":{}}"#).unwrap();
        let settings = ProjectSettings::load_at(Some(path));
        assert_eq!(
            settings.workspace(temporary.path()),
            WorkspaceState::default()
        );
    }

    #[test]
    fn workspace_state_round_trips_tabs_filters_and_review_state() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("settings.json");
        let root = temporary.path().join("repository");
        fs::create_dir_all(&root).unwrap();
        let mut state = WorkspaceState::default();
        state.file_filters.push("**/*.snap".into());
        state.hide_deleted_entries = true;
        state.tabs.push(StoredTab {
            path: "src/lib.rs".into(),
            pinned: true,
        });
        state.reviewed.insert("item".into(), "fingerprint".into());
        let mut settings = ProjectSettings::at_path(path.clone());
        settings.save(&root, state.clone()).unwrap();

        let store: StoreFile = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(store.workspaces[&repository_key(&root)].tabs, state.tabs);
        assert_eq!(
            store.workspaces[&repository_key(&root)].file_filters,
            state.file_filters
        );
        assert_eq!(
            store.workspaces[&repository_key(&root)].reviewed,
            state.reviewed
        );
        assert_eq!(ProjectSettings::load_at(Some(path)).workspace(&root), state);
    }
}
