//! Native, read-only review workspace built on Zed's GPUI rendering layer.
//!
//! This module deliberately owns only desktop presentation. Diff acquisition and
//! paired-line calculation remain in Luminatti's existing diff modules.
mod branch_status;
mod comments;
mod comments_view;
mod filter_input;
mod layout;
mod lsp;
mod model;
mod project_settings;
mod radar;
mod radar_view;
mod radar_bubble_view;
mod recent_repositories;
mod semantic;
mod theme;
mod updater;
mod view;

use std::collections::{HashMap, HashSet};
use std::borrow::Cow;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use git2::{BranchType, Repository, StatusOptions};
use globset::GlobMatcher;
use gpui::{
    div, point, prelude::*, px, size, svg, App, Application, AssetSource, Bounds, ClickEvent, Context, CursorStyle,
    Entity, KeyBinding, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathPromptOptions, Pixels, PromptLevel, Render, ScrollHandle, ScrollStrategy,
    SystemMenuType, TitlebarOptions, UniformListScrollHandle, Window, WindowBounds, WindowOptions,
};

/// Built-in icons keep the desktop binary independent of its current directory.
struct DesktopAssets;

impl AssetSource for DesktopAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let asset = match path {
            "icons/arrow_down.svg" => include_bytes!("../../assets/icons/arrow_down.svg").as_slice(),
            "icons/arrow_up.svg" => include_bytes!("../../assets/icons/arrow_up.svg").as_slice(),
            "icons/collapse_unchanged.svg" => include_bytes!("../../assets/icons/collapse_unchanged.svg").as_slice(),
            "icons/chevron_down.svg" => include_bytes!("../../assets/icons/chevron_down.svg").as_slice(),
            "icons/comment.svg" => include_bytes!("../../assets/icons/comment.svg").as_slice(),
            "icons/copy.svg" => include_bytes!("../../assets/icons/copy.svg").as_slice(),
            "icons/expand_unchanged.svg" => include_bytes!("../../assets/icons/expand_unchanged.svg").as_slice(),
            "icons/filter.svg" => include_bytes!("../../assets/icons/filter.svg").as_slice(),
            "icons/fit_view.svg" => include_bytes!("../../assets/icons/fit_view.svg").as_slice(),
            "icons/focus.svg" => include_bytes!("../../assets/icons/focus.svg").as_slice(),
            "icons/folder.svg" => include_bytes!("../../assets/icons/folder.svg").as_slice(),
            "icons/folder_open.svg" => include_bytes!("../../assets/icons/folder_open.svg").as_slice(),
            "icons/git_branch.svg" => include_bytes!("../../assets/icons/git_branch.svg").as_slice(),
            "icons/log.svg" => include_bytes!("../../assets/icons/log.svg").as_slice(),
            "icons/settings.svg" => include_bytes!("../../assets/icons/settings.svg").as_slice(),
            "icons/threads_sidebar_left_closed.svg" => include_bytes!("../../assets/icons/threads_sidebar_left_closed.svg").as_slice(),
            "icons/threads_sidebar_left_open.svg" => include_bytes!("../../assets/icons/threads_sidebar_left_open.svg").as_slice(),
            "icons/trash.svg" => include_bytes!("../../assets/icons/trash.svg").as_slice(),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(asset)))
    }

    fn list(&self, _: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        Ok(Vec::new())
    }
}

use filter_input::{FileFilterInput, FileFilterInputEvent};

use crate::command::diff::{
    diff_algo::count_added_removed,
    git::{get_changed_files, load_file_diff},
    DiffOptions,
};
use crate::commit_reference::CommitReference;
use crate::vcs::{GitBackend, VcsBackend};
use layout::{
    build_file_tree_entries, change_start_rows, display_rows_for_file, display_rows_from_sections,
    remap_tabs_after_refresh,
    logical_unchanged_sections, DiffDisplayRow, FileTreeEntry, LogTab, ReviewTab, UnchangedSection,
};
#[cfg(test)]
use layout::unchanged_sections;
use lsp::{LspCommand, LspEvent, LspStatus, LspSymbol, ReferenceTarget};
use model::{NativeRow, ReviewModel};
use project_settings::{ProjectSettings, WorkspaceState};
use recent_repositories::{resolve_repository_root, RecentRepositories};
use semantic::is_typescript_path;
use theme::{rgb, Theme, UserProfile};

gpui::actions!(desktop, [
    DismissMenus,
    OpenRepository,
    Quit,
    HideApp,
    HideOtherApps,
    ShowAllApps,
    MinimizeWindow,
    Cut,
    Copy,
    Paste,
    SelectAll,
]);

fn install_app_menus(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOtherApps, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());

    cx.set_menus(vec![
        Menu {
            name: "Luminatti".into(),
            items: vec![
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide Luminatti", HideApp),
                MenuItem::action("Hide Others", HideOtherApps),
                MenuItem::action("Show All", ShowAllApps),
                MenuItem::separator(),
                MenuItem::action("Quit Luminatti", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![MenuItem::action("Open Repository…", OpenRepository)],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Cut", Cut, gpui::OsAction::Cut),
                MenuItem::os_action("Copy", Copy, gpui::OsAction::Copy),
                MenuItem::os_action("Paste", Paste, gpui::OsAction::Paste),
                MenuItem::separator(),
                MenuItem::os_action("Select All", SelectAll, gpui::OsAction::SelectAll),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![MenuItem::action("Minimize", MinimizeWindow)],
        },
    ]);
}

// Slate palette aligned with the native review design: tabs are slightly
// lighter than the content surface, with low-contrast blue-grey dividers.
const BG: u32 = 0x282d36;
const PANEL: u32 = 0x2f3540;
const BORDER: u32 = 0x3b4350;
const TEXT: u32 = 0xc9d1d9;
const MUTED: u32 = 0x9da7b6;
const GREEN: u32 = 0x79c99e;
const RED: u32 = 0xf18c96;
const YELLOW: u32 = 0xe5b567;
const BLUE: u32 = 0x7aa2f7;
const TREE_INDENT_GUIDE: u32 = 0x3d4654;
// The active file should read as a destination in the sidebar, without competing
// with the diff's addition/deletion colors.
const ACTIVE_FILE_BG: u32 = 0x384863;
const ACTIVE_FILE_HOVER_BG: u32 = 0x405271;
// Keep a visible divider and a sliver of the diff canvas at either extreme,
// while otherwise allowing the sidebar to use the whole viewport.
const MIN_SIDEBAR_WIDTH: f32 = 4.;
const SIDEBAR_VIEWPORT_GUTTER: f32 = 4.;

fn sidebar_width_bounds(viewport_width: Pixels) -> (Pixels, Pixels) {
    let maximum = (viewport_width - px(SIDEBAR_VIEWPORT_GUTTER)).max(px(0.));
    (px(MIN_SIDEBAR_WIDTH).min(maximum), maximum)
}

fn constrain_sidebar_width(width: Pixels, viewport_width: Pixels) -> Pixels {
    let (minimum, maximum) = sidebar_width_bounds(viewport_width);
    width.clamp(minimum, maximum)
}

/// The sidebar order is independent from the order reported by the VCS, which
/// can change whenever the working tree is refreshed.
fn sort_files_alphabetically(files: &mut [model::NativeFile]) {
    files.sort_by(|left, right| left.path.cmp(&right.path));
}

/// List the files visible in the repository, including tracked and untracked
/// files while respecting Git's ignore rules. Changed paths are merged back in
/// so a deleted file remains visible when the full repository view is enabled.
fn load_repository_file_paths(
    repository_root: &Path,
    changed_files: &[model::NativeFile],
) -> Vec<String> {
    let mut paths = Command::new("git")
        .current_dir(repository_root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .filter_map(|path| String::from_utf8(path.to_vec()).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    paths.extend(changed_files.iter().map(|file| file.path.clone()));
    paths.sort();
    paths.dedup();
    paths
}

fn file_matches_change_types(
    file: &model::NativeFile,
    filters: &HashSet<view::ReviewChangeFamily>,
) -> bool {
    filters.is_empty()
        || view::ReviewChangeFamily::for_file(file)
            .into_iter()
            .any(|family| filters.contains(&family))
}

/// Build the sidebar immediately from Git's path list. The selected file is
/// resolved into a `FileDiff` later, on demand.
fn load_file_summaries(
    options: &DiffOptions,
    backend: &dyn VcsBackend,
    repository_root: &Path,
) -> Vec<model::NativeFile> {
    if options.reference.is_none() {
        let mut status_options = StatusOptions::new();
        status_options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        if let Ok(repository) = Repository::discover(repository_root) {
            if let Ok(statuses) = repository.statuses(Some(&mut status_options)) {
                return statuses
                    .iter()
                    .filter_map(|entry| {
                        let path = entry.path()?.to_string();
                        let status = entry.status();
                        let status = if status.is_wt_new() || status.is_index_new() {
                            crate::command::diff::types::FileStatus::Added
                        } else if status.is_wt_deleted() || status.is_index_deleted() {
                            crate::command::diff::types::FileStatus::Deleted
                        } else {
                            crate::command::diff::types::FileStatus::Modified
                        };
                        Some(model::NativeFile::summary(path, status))
                    })
                    .collect();
            }
        }
    }
    get_changed_files(options, backend)
        .into_iter()
        .map(|path| {
            model::NativeFile::summary(path, crate::command::diff::types::FileStatus::Modified)
        })
        .collect()
}

/// Git can calculate aggregate line statistics without materializing the file
/// contents or paired rows. Run this after the first paint so it never holds
/// up sidebar navigation.
fn load_working_copy_numstat(repository_root: &Path) -> HashMap<String, (usize, usize)> {
    let mut counts = HashMap::new();
    let output = Command::new("git")
        .current_dir(repository_root)
        .args(["diff", "--numstat", "HEAD", "--"])
        .output();
    if let Ok(output) = output {
        if output.status.success() {
            counts.extend(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter_map(|line| {
                        let mut parts = line.splitn(3, '\t');
                        let additions = parts.next()?.parse().ok()?;
                        let deletions = parts.next()?.parse().ok()?;
                        let path = parts.next()?.to_string();
                        Some((path, (additions, deletions)))
                    }),
            );
        }
    }

    // `git diff HEAD --numstat` intentionally excludes untracked files. Count
    // their contents separately so newly added files do not remain at +0/-0.
    if let Ok(output) = Command::new("git")
        .current_dir(repository_root)
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .output()
    {
        if output.status.success() {
            for path in output.stdout.split(|byte| *byte == b'\0') {
                let Ok(path) = std::str::from_utf8(path) else {
                    continue;
                };
                if path.is_empty() {
                    continue;
                }
                if let Ok(contents) = std::fs::read_to_string(repository_root.join(path)) {
                    let (additions, _) = count_added_removed("", &contents);
                    counts.insert(path.to_string(), (additions, 0));
                }
            }
        }
    }

    counts
}

fn observed_at_millis(path: &Path) -> u128 {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn load_typescript_analyses(
    repository_root: &Path,
    reference: Option<CommitReference>,
    paths: Vec<String>,
) -> Vec<model::NativeFile> {
    let Ok(backend) = GitBackend::new(repository_root) else {
        return Vec::new();
    };
    let options = DiffOptions {
        reference,
        pr: None,
        detect_pr: false,
        file: None,
        watch: false,
        theme: None,
        stacked: false,
        focus: None,
        origin: None,
        wrap: false,
    };
    paths
        .into_iter()
        .filter(|path| is_typescript_path(path))
        .map(|path| {
            let timestamp = observed_at_millis(&repository_root.join(&path));
            model::NativeFile::from_diff(&load_file_diff(path, &options, &backend))
                .with_observed_at(timestamp)
        })
        .collect()
}

fn review_commit_id(reference: Option<&CommitReference>, backend: &dyn VcsBackend) -> String {
    let reference = match reference {
        Some(CommitReference::Single(reference)) => reference.as_str(),
        Some(CommitReference::Range { to, .. } | CommitReference::TripleDots { to, .. }) => {
            to.as_str()
        }
        Some(CommitReference::RangeToWorkingTree { .. }) | None => {
            backend.working_copy_parent_ref()
        }
    };

    backend
        .resolve_ref(reference)
        .unwrap_or_else(|_| reference.to_string())
        .chars()
        .take(8)
        .collect()
}

fn is_path_filtered(path: &str, matchers: &[GlobMatcher]) -> bool {
    matchers.iter().any(|matcher| matcher.is_match(path))
}

fn hide_unchanged_sections_for_file(
    file: &model::NativeFile,
    collapsed_sections: &mut HashSet<UnchangedSection>,
    symbols: &[LspSymbol],
) {
    collapsed_sections.extend(logical_unchanged_sections(file, symbols));
}

fn project_name() -> String {
    project_name_for(&project_root())
}

fn project_name_for(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .to_string()
}

fn abbreviated_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(relative) = path.strip_prefix(home) {
            return format!("~/{}", relative.display());
        }
    }
    path.display().to_string()
}

fn project_root() -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    cwd.ancestors()
        .find(|path| path.join(".git").exists())
        .unwrap_or(cwd.as_path())
        .canonicalize()
        .unwrap_or(cwd)
}

fn repository_branches(repository_root: &Path) -> (Vec<String>, Vec<String>) {
    let Ok(repository) = Repository::discover(repository_root) else {
        return (Vec::new(), Vec::new());
    };

    let branch_names = |branch_type| {
        let Ok(branches) = repository.branches(Some(branch_type)) else {
            return Vec::new();
        };
        let mut names = branches
            .filter_map(Result::ok)
            .filter_map(|(branch, _)| branch.name().ok().flatten().map(str::to_string))
            // A remote's symbolic HEAD is not a branch that can be checked out.
            .filter(|name| !name.ends_with("/HEAD"))
            .collect::<Vec<_>>();
        names.sort_by_key(|name| name.to_lowercase());
        names
    };

    (
        branch_names(BranchType::Local),
        branch_names(BranchType::Remote),
    )
}

pub fn run(
    reference: Option<CommitReference>,
    focus: Option<String>,
    backend: Box<dyn VcsBackend>,
) -> io::Result<()> {
    let repository_root = project_root();
    let project_name = project_name();
    let branch_name = backend
        .get_current_branch()
        .ok()
        .flatten()
        .unwrap_or_else(|| "detached HEAD".to_string());
    let commit_id = review_commit_id(reference.as_ref(), backend.as_ref());
    let is_working_copy_review = reference.is_none();
    let options = DiffOptions {
        reference,
        pr: None,
        detect_pr: false,
        file: None,
        watch: false,
        theme: None,
        stacked: false,
        focus: focus.clone(),
        origin: None,
        wrap: false,
    };
    let mut files = load_file_summaries(&options, backend.as_ref(), &repository_root);
    sort_files_alphabetically(&mut files);
    let semantic_paths = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let review = ReviewModel::new(files, focus.as_deref());
    let workspace_reference = options.reference.clone();
    let workspace_root = repository_root.clone();
    let (watcher, watch_events, workspace_git_dir) = setup_workspace_watcher(&workspace_root)?;
    let (count_sender, count_events) = mpsc::channel();
    let (semantic_sender, semantic_events) = mpsc::channel();
    {
        let sender = semantic_sender.clone();
        let root = workspace_root.clone();
        let reference = workspace_reference.clone();
        std::thread::spawn(move || {
            let analyses = load_typescript_analyses(&root, reference, semantic_paths);
            let _ = sender.send((root, analyses));
        });
    }
    let (lsp_sender, lsp_events) = lsp::spawn(workspace_root.clone());
    if is_working_copy_review {
        let count_root = workspace_root.clone();
        let sender = count_sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send((count_root.clone(), load_working_copy_numstat(&count_root)));
        });
    }
    let mut recent_repositories = RecentRepositories::load();
    let _ = recent_repositories.record(&repository_root);

    Application::new().with_assets(DesktopAssets).run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("cmd-o", OpenRepository, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", HideApp, None),
            KeyBinding::new("cmd-alt-h", HideOtherApps, None),
            KeyBinding::new("cmd-m", MinimizeWindow, None),
            KeyBinding::new("cmd-x", Cut, None),
            KeyBinding::new("cmd-c", Copy, None),
            KeyBinding::new("cmd-v", Paste, None),
            KeyBinding::new("cmd-a", SelectAll, None),
            KeyBinding::new("escape", DismissMenus, None),
        ]);
        install_app_menus(cx);
        let bounds = Bounds::centered(None, size(px(1440.), px(920.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(format!("{}  ·  {}", project_name, branch_name).into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    cx.new(|cx| {
                        ReviewWorkspace::new(
                            window,
                            cx,
                            review,
                            project_name,
                            branch_name,
                            commit_id,
                            repository_root,
                            workspace_reference,
                            watcher,
                            watch_events,
                            workspace_git_dir,
                            count_sender,
                            count_events,
                            semantic_sender,
                            semantic_events,
                            lsp_sender,
                            lsp_events,
                            recent_repositories,
                            focus,
                        )
                    })
                },
            )
            .map_err(|error| io::Error::other(error.to_string()))
            .unwrap();

        // Poll repository-specific channels through the workspace entity. A
        // repository switch replaces those channels and its watcher in-place.
        cx.spawn(async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            if window
                .update(cx, |workspace, window, cx| {
                    workspace.handle_background_events(window, cx)
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}

/// Owns the debouncer. Dropping it stops the filesystem watcher.
#[allow(dead_code)]
struct WorkspaceWatcher(notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>);

fn setup_workspace_watcher(
    repository_root: &Path,
) -> io::Result<(WorkspaceWatcher, Receiver<Vec<PathBuf>>, PathBuf)> {
    use notify_debouncer_mini::new_debouncer;

    let (tx, rx) = mpsc::channel();
    let repository = Repository::discover(repository_root).ok();
    let git_dir = repository
        .as_ref()
        .map(|repository| repository.path().to_path_buf())
        .unwrap_or_else(|| repository_root.join(".git"));
    let mut debouncer = new_debouncer(
        Duration::from_millis(350),
        move |result: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
            if let Ok(events) = result {
                let paths = events.into_iter().map(|event| event.path).collect::<Vec<_>>();
                if !paths.is_empty() {
                    let _ = tx.send(paths);
                }
            }
        },
    )
    .map_err(io::Error::other)?;

    debouncer
        .watcher()
        .watch(repository_root, notify::RecursiveMode::Recursive)
        .map_err(io::Error::other)?;

    // In a linked worktree `.git` is a file in the working tree, while the
    // actual Git directory lives outside it. Watch that directory as well so
    // commits, index updates, and branch switches also refresh the counter.
    if !git_dir.starts_with(repository_root) {
            debouncer
                .watcher()
                .watch(&git_dir, notify::RecursiveMode::Recursive)
                .map_err(io::Error::other)?;
    }

    Ok((WorkspaceWatcher(debouncer), rx, git_dir))
}

/// Ignore filesystem activity that cannot change the current comparison. In
/// particular, Git's object database and ignored build outputs are noisy on
/// macOS and previously caused each notification to restart review analysis.
fn workspace_event_can_change_review(
    path: &Path,
    repository_root: &Path,
    git_dir: &Path,
    repository: &Repository,
) -> bool {
    if let Ok(relative) = path.strip_prefix(git_dir) {
        return matches!(relative.to_str(), Some("HEAD" | "index" | "packed-refs"))
            || relative.starts_with("refs/heads");
    }

    let Ok(relative) = path.strip_prefix(repository_root) else {
        return false;
    };
    if relative.as_os_str().is_empty()
        || matches!(relative.components().next(), Some(Component::Normal(name)) if name == ".git")
    {
        return false;
    }

    // If ignore matching fails (for example, while a file is being renamed),
    // refresh conservatively rather than missing a genuine source change.
    !repository.status_should_ignore(relative).unwrap_or(false)
}

struct ReviewWorkspace {
    model: ReviewModel,
    repository_files: Vec<String>,
    tabs: Vec<ReviewTab>,
    log_tabs: Vec<LogTab>,
    active_log: Option<LogTab>,
    radar: radar::RadarState,
    radar_bubbles: radar::bubbles::BubbleState,
    review_comments: comments::State,
    comment_input: Entity<FileFilterInput>,
    project_name: String,
    branch_name: String,
    branch_statuses: HashMap<String, branch_status::BranchStatus>,
    branch_status_task: Option<Receiver<(PathBuf, HashMap<String, branch_status::BranchStatus>)>>,
    last_branch_status_refresh: Instant,
    commit_id: String,
    repository_root: PathBuf,
    reference: Option<CommitReference>,
    sidebar_width: Pixels,
    filtered_panel_height: Pixels,
    annotation_width: Pixels,
    outline_width: Pixels,
    diff_left_width: Pixels,
    diff_view: DiffView,
    show_sidebar: bool,
    show_annotations: bool,
    active_file: Option<model::NativeFile>,
    active_file_load: Option<(PathBuf, String, Receiver<Option<model::NativeFile>>)>,
    file_view: FileView,
    show_only_changes: bool,
    hide_deleted_entries: bool,
    show_file_view_menu: bool,
    collapsed_directories: HashSet<String>,
    reviewed_files: HashSet<String>,
    change_type_filters: HashSet<view::ReviewChangeFamily>,
    context_paths: HashSet<String>,
    /// Glob patterns shared by Changes, the semantic summary, and Radar.
    file_filters: Vec<String>,
    file_filter_matchers: Vec<GlobMatcher>,
    file_filter_input: Entity<FileFilterInput>,
    file_filter_error: Option<String>,
    show_file_filter: bool,
    left_code_scroll: ScrollHandle,
    right_code_scroll: ScrollHandle,
    diff_list_scroll: UniformListScrollHandle,
    unified_diff_scroll: UniformListScrollHandle,
    selected_change: usize,
    collapsed_unchanged_sections: HashSet<UnchangedSection>,
    hide_unchanged_sections: bool,
    drag: Option<DragKind>,
    model_revision: u64,
    show_project_menu: bool,
    show_branch_menu: bool,
    show_settings_menu: bool,
    user_profile: UserProfile,
    project_settings: ProjectSettings,
    recent_repositories: RecentRepositories,
    _workspace_watcher: WorkspaceWatcher,
    workspace_watch_events: Receiver<Vec<PathBuf>>,
    workspace_git_dir: PathBuf,
    count_sender: mpsc::Sender<(PathBuf, HashMap<String, (usize, usize)>)>,
    count_events: Receiver<(PathBuf, HashMap<String, (usize, usize)>)>,
    semantic_sender: mpsc::Sender<(PathBuf, Vec<model::NativeFile>)>,
    semantic_events: Receiver<(PathBuf, Vec<model::NativeFile>)>,
    lsp_sender: mpsc::Sender<LspCommand>,
    lsp_events: Receiver<LspEvent>,
    update_events: Receiver<updater::Event>,
    available_update: Option<updater::Release>,
    lsp_status: LspStatus,
    lsp_logs: Vec<String>,
    status_pulse_visible: bool,
    last_status_pulse: Instant,
    lsp_symbols: HashMap<String, Vec<LspSymbol>>,
    lsp_references: HashMap<String, HashMap<String, usize>>,
    show_lsp_menu: bool,
    show_outline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum FileView {
    Flat,
    Tree,
    CompactTree,
}

/// The presentation of a file diff. Split is the default to preserve the
/// existing side-by-side review workflow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum DiffView {
    Unified,
    #[default]
    Split,
}

#[derive(Clone)]
struct SidebarFileEntry {
    path: String,
    model_index: Option<usize>,
    is_changed: bool,
}

#[derive(Clone, Copy)]
enum DragKind {
    Sidebar { start: Pixels, width: Pixels },
    FilteredPanel { start: Pixels, height: Pixels },
    Annotations { start: Pixels, width: Pixels },
    Outline { start: Pixels, width: Pixels },
    Diff { start: Pixels, width: Pixels },
    LeftCodeScroll { start: Pixels, offset: Pixels },
    RightCodeScroll { start: Pixels, offset: Pixels },
}

impl ReviewWorkspace {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        model: ReviewModel,
        project_name: String,
        branch_name: String,
        commit_id: String,
        repository_root: PathBuf,
        reference: Option<CommitReference>,
        workspace_watcher: WorkspaceWatcher,
        workspace_watch_events: Receiver<Vec<PathBuf>>,
        workspace_git_dir: PathBuf,
        count_sender: mpsc::Sender<(PathBuf, HashMap<String, (usize, usize)>)>,
        count_events: Receiver<(PathBuf, HashMap<String, (usize, usize)>)>,
        semantic_sender: mpsc::Sender<(PathBuf, Vec<model::NativeFile>)>,
        semantic_events: Receiver<(PathBuf, Vec<model::NativeFile>)>,
        lsp_sender: mpsc::Sender<LspCommand>,
        lsp_events: Receiver<LspEvent>,
        recent_repositories: RecentRepositories,
        explicit_focus: Option<String>,
    ) -> Self {
        let file_filter_input = cx.new(|cx| FileFilterInput::new(window, cx));
        cx.subscribe(
            &file_filter_input,
            |this, _, event: &FileFilterInputEvent, cx| match event {
                FileFilterInputEvent::Submit(pattern) => {
                    this.add_file_filter(pattern, cx);
                }
                FileFilterInputEvent::Invalid(error) => {
                    this.file_filter_error = Some(error.clone());
                    cx.notify();
                }
                FileFilterInputEvent::Changed => {
                    if this.file_filter_error.take().is_some() {
                        cx.notify();
                    }
                }
                FileFilterInputEvent::Dismiss => {
                    this.show_file_filter = false;
                    this.file_filter_error = None;
                    this.persist_project_settings();
                    cx.notify();
                }
            },
        )
        .detach();
        let comment_input = cx.new(|cx| FileFilterInput::new_comment(window, cx));
        cx.subscribe(&comment_input, |this, _, event: &FileFilterInputEvent, cx| {
            match event {
                FileFilterInputEvent::Submit(body) => this.save_review_comment(body, cx),
                FileFilterInputEvent::Dismiss => this.cancel_review_comment(cx),
                _ => cx.notify(),
            }
        }).detach();
        let (update_sender, update_events) = mpsc::channel();
        updater::check_for_update(update_sender, env!("CARGO_PKG_VERSION"));
        let repository_files = load_repository_file_paths(&repository_root, &model.files);
        let tabs = (!model.files.is_empty())
            .then_some(ReviewTab {
                file_index: model.selected,
                is_preview: true,
            })
            .into_iter()
            .collect();
        let user_profile = UserProfile::load();
        user_profile.theme.activate();
        let mut workspace = Self {
            model,
            repository_files,
            tabs,
            log_tabs: Vec::new(),
            active_log: None,
            radar: radar::RadarState::default(),
            radar_bubbles: radar::bubbles::BubbleState::default(),
            review_comments: comments::State::default(),
            comment_input,
            project_name,
            branch_name,
            branch_statuses: HashMap::new(),
            branch_status_task: None,
            last_branch_status_refresh: Instant::now(),
            commit_id,
            repository_root,
            reference,
            sidebar_width: px(272.),
            filtered_panel_height: px(180.),
            annotation_width: px(300.),
            outline_width: px(280.),
            diff_left_width: px(520.),
            diff_view: DiffView::Split,
            show_sidebar: true,
            show_annotations: false,
            active_file: None,
            active_file_load: None,
            file_view: FileView::CompactTree,
            show_only_changes: true,
            hide_deleted_entries: false,
            show_file_view_menu: false,
            collapsed_directories: HashSet::new(),
            reviewed_files: HashSet::new(),
            change_type_filters: HashSet::new(),
            context_paths: HashSet::new(),
            file_filters: Vec::new(),
            file_filter_matchers: Vec::new(),
            file_filter_input,
            file_filter_error: None,
            show_file_filter: false,
            left_code_scroll: ScrollHandle::new(),
            right_code_scroll: ScrollHandle::new(),
            diff_list_scroll: UniformListScrollHandle::new(),
            unified_diff_scroll: UniformListScrollHandle::new(),
            selected_change: 0,
            collapsed_unchanged_sections: HashSet::new(),
            hide_unchanged_sections: false,
            drag: None,
            model_revision: 0,
            show_project_menu: false,
            show_branch_menu: false,
            show_settings_menu: false,
            user_profile,
            project_settings: ProjectSettings::load(),
            recent_repositories,
            _workspace_watcher: workspace_watcher,
            workspace_watch_events,
            workspace_git_dir,
            count_sender,
            count_events,
            semantic_sender,
            semantic_events,
            lsp_sender,
            lsp_events,
            update_events,
            available_update: None,
            lsp_status: LspStatus::Idle,
            lsp_logs: vec!["Waiting for a TypeScript file.".into()],
            status_pulse_visible: true,
            last_status_pulse: Instant::now(),
            lsp_symbols: HashMap::new(),
            lsp_references: HashMap::new(),
            show_lsp_menu: false,
            show_outline: false,
        };
        workspace.refresh_branch_status();
        workspace.restore_project_settings(explicit_focus.as_deref(), cx);
        workspace.rebuild_active_file();
        workspace.refresh_radar();
        workspace.request_lsp_for_active_file();
        workspace
    }

    fn saved_workspace_state(&self) -> WorkspaceState {
        WorkspaceState {
            sidebar_width: Some(f32::from(self.sidebar_width).max(0.) as u32),
            filtered_panel_height: Some(f32::from(self.filtered_panel_height).max(0.) as u32),
            annotation_width: Some(f32::from(self.annotation_width).max(0.) as u32),
            outline_width: Some(f32::from(self.outline_width).max(0.) as u32),
            diff_left_width: Some(f32::from(self.diff_left_width).max(0.) as u32),
            diff_view: self.diff_view,
            show_sidebar: self.show_sidebar,
            show_annotations: self.show_annotations,
            show_outline: self.show_outline,
            file_view: self.file_view,
            show_only_changes: self.show_only_changes,
            hide_deleted_entries: self.hide_deleted_entries,
            show_file_filter: self.show_file_filter,
            collapsed_directories: self.collapsed_directories.iter().cloned().collect(),
            change_type_filters: self.change_type_filters.iter().copied().collect(),
            file_filters: self.file_filters.clone(),
            hide_unchanged_sections: self.hide_unchanged_sections,
            collapsed_unchanged_sections: self
                .collapsed_unchanged_sections
                .iter()
                .cloned()
                .collect(),
            tabs: self
                .tabs
                .iter()
                .filter_map(|tab| {
                    self.model.files.get(tab.file_index).map(|file| project_settings::StoredTab {
                        path: file.path.clone(),
                        pinned: !tab.is_preview,
                    })
                })
                .collect(),
            selected_file: self
                .model
                .files
                .get(self.model.selected)
                .map(|file| file.path.clone()),
            selected_change: self.selected_change,
            reviewed_files: self.reviewed_files.iter().cloned().collect(),
            radar_open: self.radar.open,
            radar_active: self.radar.active,
            radar_expand_all: self.radar.expand_all,
            radar_expanded: self.radar.expanded.clone(),
            radar_cycles: self.radar.cycles.clone(),
            radar_expanded_files: self.radar.expanded_files.clone(),
            radar_focused_files: self.radar.focused_files.clone(),
        }
    }

    fn persist_project_settings(&mut self) {
        let root = self.repository_root.clone();
        let state = self.saved_workspace_state();
        let _ = self.project_settings.save(&root, state);
    }

    fn ensure_workspace_file(&mut self, path: &str) -> Option<usize> {
        if let Some(index) = self.model.files.iter().position(|file| file.path == path) {
            return Some(index);
        }
        if !self.repository_files.iter().any(|candidate| candidate == path) {
            return None;
        }
        let file = model::NativeFile::summary(path.to_string(), crate::command::diff::types::FileStatus::Modified);
        self.context_paths.insert(path.to_string());
        self.model.files.push(file);
        Some(self.model.files.len() - 1)
    }

    fn restore_project_settings(&mut self, explicit_focus: Option<&str>, cx: &mut Context<Self>) {
        let state = self.project_settings.workspace(&self.repository_root);
        self.sidebar_width = px(state.sidebar_width.unwrap_or(272) as f32);
        self.filtered_panel_height = px(state.filtered_panel_height.unwrap_or(180).clamp(80, 500) as f32);
        self.annotation_width = px(state.annotation_width.unwrap_or(300).clamp(220, 480) as f32);
        self.outline_width = px(state.outline_width.unwrap_or(280).clamp(220, 480) as f32);
        self.diff_left_width = px(state.diff_left_width.unwrap_or(520).clamp(240, 1200) as f32);
        self.diff_view = state.diff_view;
        self.show_sidebar = state.show_sidebar;
        self.show_annotations = state.show_annotations;
        self.show_outline = state.show_outline;
        self.file_view = state.file_view;
        self.show_only_changes = state.show_only_changes;
        self.hide_deleted_entries = state.hide_deleted_entries;
        self.show_file_filter = state.show_file_filter;
        self.collapsed_directories = state.collapsed_directories.into_iter().collect();
        self.change_type_filters = state.change_type_filters.into_iter().collect();
        self.file_filters.clear();
        self.file_filter_matchers.clear();
        for pattern in state.file_filters {
            if let Ok(glob) = globset::Glob::new(&pattern) {
                self.file_filter_matchers.push(glob.compile_matcher());
                self.file_filters.push(pattern);
            }
        }
        self.hide_unchanged_sections = state.hide_unchanged_sections;
        self.collapsed_unchanged_sections = state.collapsed_unchanged_sections.into_iter().collect();
        self.selected_change = state.selected_change;
        self.reviewed_files = state.reviewed_files.into_iter().collect();
        self.radar.open = state.radar_open;
        self.radar.active = state.radar_open && state.radar_active && explicit_focus.is_none();
        self.radar.expand_all = state.radar_expand_all;
        self.radar.expanded = state.radar_expanded;
        self.radar.cycles = state.radar_cycles;
        self.radar.expanded_files = state.radar_expanded_files;
        self.radar.focused_files = state.radar_focused_files;

        self.tabs.clear();
        for saved_tab in state.tabs {
            let Some(index) = self.ensure_workspace_file(&saved_tab.path) else {
                continue;
            };
            if !self.tabs.iter().any(|tab| tab.file_index == index) {
                self.tabs.push(ReviewTab {
                    file_index: index,
                    is_preview: !saved_tab.pinned,
                });
            }
        }
        let selected_path = explicit_focus
            .map(str::to_string)
            .or(state.selected_file)
            .and_then(|path| self.ensure_workspace_file(&path));
        if let Some(selected) = selected_path {
            if !self.tabs.iter().any(|tab| tab.file_index == selected) {
                self.tabs.push(ReviewTab {
                    file_index: selected,
                    is_preview: true,
                });
            }
            self.model.selected = selected;
        } else if let Some(tab) = self.tabs.first() {
            self.model.selected = tab.file_index;
        } else if !self.model.files.is_empty() {
            self.tabs.push(ReviewTab {
                file_index: self.model.selected,
                is_preview: true,
            });
        }
        if explicit_focus.is_some() {
            self.radar.active = false;
            self.review_comments.active = false;
        }
        self.collapsed_unchanged_sections.retain(|section| {
            self.repository_files
                .iter()
                .any(|path| path == &section.file_path)
                || self
                    .model
                    .files
                    .iter()
                    .any(|file| file.path == section.file_path)
        });
        self.file_filter_input.update(cx, |input, cx| input.clear(cx));
    }

    fn reload_files(
        &mut self,
        mut files: Vec<model::NativeFile>,
        commit_id: String,
        branch_name: String,
        cx: &mut Context<Self>,
    ) {
        // Refreshes start with lightweight file summaries. Retain the last
        // known stats until the background numstat task supplies fresh values,
        // avoiding a visible +0/-0 flash on every filesystem event.
        let previous_counts = self
            .model
            .files
            .iter()
            .map(|file| (file.path.clone(), (file.additions, file.deletions)))
            .collect::<HashMap<_, _>>();
        for file in &mut files {
            if let Some((additions, deletions)) = previous_counts.get(&file.path) {
                file.additions = *additions;
                file.deletions = *deletions;
            }
        }
        let (tabs, selected) =
            remap_tabs_after_refresh(&self.model.files, &self.tabs, self.model.selected, &files);
        self.repository_files = load_repository_file_paths(&self.repository_root, &files);
        // `files` comes directly from Git's changed-path list. Any context
        // classification belongs to the previous model and must not hide a
        // path that has since become a real change.
        self.context_paths.clear();
        self.model.replace_files(files);
        self.active_file_load = None;
        self.model.selected = selected;
        self.tabs = tabs;
        self.refresh_radar();
        self.commit_id = commit_id;
        self.branch_name = branch_name;
        self.refresh_branch_status();
        self.selected_change = 0;
        self.collapsed_unchanged_sections.clear();
        self.rebuild_active_file();
        self.request_lsp_for_active_file();
        self.diff_list_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        self.model_revision = self.model_revision.wrapping_add(1);
        cx.notify();
    }

    fn rebuild_active_file(&mut self) {
        // A numeric selection can outlive a closed tab. Only an open tab may
        // supply the active file shown in the canvas or highlighted sidebar.
        if !self.is_file_selected(self.model.selected) {
            self.active_file = None;
            return;
        }
        let Some(summary) = self.model.files.get(self.model.selected).cloned() else {
            self.active_file = None;
            return;
        };
        // Full rows remain on the lightweight sidebar record once loaded. Use
        // them when revisiting a tab instead of reopening the repository and
        // rebuilding the side-by-side diff on the UI thread.
        if !summary.review_fingerprint.is_empty() {
            self.active_file_load = None;
            self.active_file = Some(summary);
            self.apply_unchanged_sections_preference();
            return;
        }
        self.active_file = None;
        if self.active_file_load.as_ref().is_some_and(|(root, path, _)| {
            root == &self.repository_root && path == &summary.path
        }) {
            return;
        }
        let root = self.repository_root.clone();
        let path = summary.path;
        let reference = self.reference.clone();
        let (sender, receiver) = mpsc::channel();
        self.active_file_load = Some((root.clone(), path.clone(), receiver));
        std::thread::spawn(move || {
            let options = DiffOptions {
                reference, pr: None, detect_pr: false, file: None, watch: false,
                theme: None, stacked: false, focus: None, origin: None, wrap: false,
            };
            let file = GitBackend::new(&root).ok().map(|backend| {
                let diff = load_file_diff(path, &options, &backend);
                let timestamp = observed_at_millis(&root.join(&diff.filename));
                model::NativeFile::from_diff(&diff).with_observed_at(timestamp)
            });
            let _ = sender.send(file);
        });
    }

    fn apply_unchanged_sections_preference(&mut self) {
        if !self.hide_unchanged_sections {
            return;
        }
        let Some(file) = self.active_file.as_ref() else {
            return;
        };
        hide_unchanged_sections_for_file(
            file,
            &mut self.collapsed_unchanged_sections,
            self.lsp_symbols.get(&file.path).map(Vec::as_slice).unwrap_or(&[]),
        );
    }

    fn apply_file_counts(
        &mut self,
        counts: HashMap<String, (usize, usize)>,
        cx: &mut Context<Self>,
    ) {
        for file in &mut self.model.files {
            if let Some((additions, deletions)) = counts.get(&file.path) {
                file.additions = *additions;
                file.deletions = *deletions;
            }
        }
        if self.radar.open {
            self.refresh_radar_bubbles();
        }
        cx.notify();
    }

    fn refresh_branch_status(&mut self) {
        let root = self.repository_root.clone();
        let (sender, receiver) = mpsc::channel();
        self.branch_status_task = Some(receiver);
        self.last_branch_status_refresh = Instant::now();
        std::thread::spawn(move || {
            let statuses = branch_status::load(&root);
            let _ = sender.send((root, statuses));
        });
    }

    fn handle_background_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((root, path, receiver)) = &self.active_file_load {
            match receiver.try_recv() {
                Ok(file) => {
                    let root = root.clone();
                    let path = path.clone();
                    self.active_file_load = None;
                    if root == self.repository_root {
                        if let (Some(index), Some(file)) = (
                            self.model.files.iter().position(|candidate| candidate.path == path), file,
                        ) {
                            self.model.files[index] = file.clone();
                            if self.is_file_selected(index) {
                                self.active_file = Some(file);
                                self.apply_unchanged_sections_preference();
                                self.request_lsp_for_active_file();
                            }
                            self.model_revision = self.model_revision.wrapping_add(1);
                            cx.notify();
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.active_file_load = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(receiver) = &self.branch_status_task {
            match receiver.try_recv() {
                Ok((root, statuses)) => {
                    self.branch_status_task = None;
                    if root == self.repository_root && statuses != self.branch_statuses {
                        self.branch_statuses = statuses;
                        cx.notify();
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.branch_status_task = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        // Also covers remote fetches and config changes in linked worktrees,
        // whose shared Git directory can live outside the workspace watcher.
        if self.branch_status_task.is_none()
            && self.last_branch_status_refresh.elapsed() >= Duration::from_secs(5)
        {
            self.refresh_branch_status();
        }
        if self.radar.poll() {
            if self.radar.open && self.radar.has_graph() {
                self.refresh_radar_bubbles();
            }
            cx.notify();
        }
        if self.radar_bubbles.poll() {
            if let Some(path) = self.radar_bubbles.selected.clone() {
                self.radar_focus(path, cx);
            }
            cx.notify();
        }
        while let Ok((repository_root, counts)) = self.count_events.try_recv() {
            if repository_root == self.repository_root {
                self.apply_file_counts(counts, cx);
            }
        }
        while let Ok((repository_root, analyses)) = self.semantic_events.try_recv() {
            if repository_root == self.repository_root {
                self.apply_semantic_analyses(analyses);
                cx.notify();
            }
        }
        while let Ok(event) = self.lsp_events.try_recv() {
            match event {
                LspEvent::Status(status) => {
                    self.push_lsp_log(status.label());
                    self.lsp_status = status;
                }
                LspEvent::Analysis {
                    path,
                    symbols,
                    references,
                } => {
                    if let Ok(relative) = path.strip_prefix(&self.repository_root) {
                        let key = relative.to_string_lossy().to_string();
                        self.push_lsp_log(format!(
                            "Analysed {key}: {} symbols, {} reference groups.",
                            symbols.len(),
                            references.len()
                        ));
                        self.lsp_symbols.insert(key.clone(), symbols);
                        self.lsp_references.insert(key, references);
                        if self.hide_unchanged_sections {
                            let active_path = self.selected_file().map(|file| file.path.clone());
                            if let Some(path) = active_path {
                                self.collapsed_unchanged_sections.retain(|section| section.file_path != path);
                                self.apply_unchanged_sections_preference();
                                self.model_revision = self.model_revision.wrapping_add(1);
                            }
                        }
                    }
                }
            }
            cx.notify();
        }
        while let Ok(event) = self.update_events.try_recv() {
            if let updater::Event::Available(release) = event {
                self.available_update = Some(release);
                cx.notify();
            }
        }
        if matches!(self.lsp_status, LspStatus::Starting)
            && self.last_status_pulse.elapsed() >= Duration::from_millis(500)
        {
            self.status_pulse_visible = !self.status_pulse_visible;
            self.last_status_pulse = Instant::now();
            cx.notify();
        } else if !matches!(self.lsp_status, LspStatus::Starting)
            && !self.status_pulse_visible
        {
            self.status_pulse_visible = true;
            cx.notify();
        }
        let repository = Repository::discover(&self.repository_root).ok();
        let mut should_reload = false;
        while let Ok(paths) = self.workspace_watch_events.try_recv() {
            should_reload |= repository.as_ref().is_some_and(|repository| {
                paths.iter().any(|path| {
                    workspace_event_can_change_review(
                        path,
                        &self.repository_root,
                        &self.workspace_git_dir,
                        repository,
                    )
                })
            });
        }
        if should_reload {
            self.reload_repository(window, cx);
        }
    }

    fn apply_semantic_analyses(&mut self, analyses: Vec<model::NativeFile>) {
        for analysis in analyses {
            if let Some(existing) = self
                .model
                .files
                .iter_mut()
                .find(|file| file.path == analysis.path)
            {
                if existing.review_fingerprint.is_empty()
                    || existing.review_fingerprint == analysis.review_fingerprint
                {
                    let additions = existing.additions;
                    let deletions = existing.deletions;
                    *existing = analysis;
                    existing.additions = additions;
                    existing.deletions = deletions;
                }
            }
        }
        if let Some(active) = self.model.files.get(self.model.selected).cloned() {
            if self.is_file_selected(self.model.selected) && !active.review_fingerprint.is_empty() {
                self.active_file = Some(active);
                self.apply_unchanged_sections_preference();
                self.request_lsp_for_active_file();
            }
        }
        self.model_revision = self.model_revision.wrapping_add(1);
    }

    fn reload_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(backend) = GitBackend::new(&self.repository_root) else {
            return;
        };
        let options = DiffOptions {
            reference: self.reference.clone(),
            pr: None,
            detect_pr: false,
            file: None,
            watch: false,
            theme: None,
            stacked: false,
            focus: None,
            origin: None,
            wrap: false,
        };
        let mut files = load_file_summaries(&options, &backend, &self.repository_root);
        sort_files_alphabetically(&mut files);
        let commit_id = review_commit_id(options.reference.as_ref(), &backend);
        let branch_name = backend
            .get_current_branch()
            .ok()
            .flatten()
            .unwrap_or_else(|| "detached HEAD".to_string());
        self.reload_files(files, commit_id, branch_name, cx);
        window.set_window_title(&self.window_title());
        self.schedule_working_copy_counts();
        self.schedule_semantic_analysis();
    }

    fn schedule_working_copy_counts(&self) {
        if self.reference.is_some() {
            return;
        }
        let repository_root = self.repository_root.clone();
        let sender = self.count_sender.clone();
        std::thread::spawn(move || {
            let counts = load_working_copy_numstat(&repository_root);
            let _ = sender.send((repository_root, counts));
        });
    }

    fn schedule_semantic_analysis(&self) {
        let repository_root = self.repository_root.clone();
        let reference = self.reference.clone();
        let paths = self
            .model
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let sender = self.semantic_sender.clone();
        std::thread::spawn(move || {
            let analyses = load_typescript_analyses(&repository_root, reference, paths);
            let _ = sender.send((repository_root, analyses));
        });
    }

    fn request_lsp_for_active_file(&self) {
        let Some(file) = self.active_file.as_ref() else {
            return;
        };
        if !is_typescript_path(&file.path) || file.new_content.is_empty() {
            return;
        }
        let targets = file
            .semantic
            .symbols
            .iter()
            .map(|symbol| ReferenceTarget {
                key: symbol.name.clone(),
                line: symbol.line,
                column: symbol.column,
            })
            .collect();
        let _ = self.lsp_sender.send(LspCommand::Analyze {
            path: self.repository_root.join(&file.path),
            source: file.new_content.to_string(),
            targets,
        });
    }

    fn window_title(&self) -> String {
        format!("{}  ·  {}", self.project_name, self.branch_name)
    }

    fn reset_workspace(
        &mut self,
        model: ReviewModel,
        project_name: String,
        branch_name: String,
        commit_id: String,
        repository_root: PathBuf,
        reference: Option<CommitReference>,
        cx: &mut Context<Self>,
    ) {
        let tabs = (!model.files.is_empty())
            .then_some(ReviewTab {
                file_index: model.selected,
                is_preview: true,
            })
            .into_iter()
            .collect();
        self.model = model;
        self.tabs = tabs;
        self.log_tabs.clear();
        self.active_log = None;
        self.lsp_logs.clear();
        self.lsp_logs.push("Waiting for a TypeScript file.".into());
        self.radar = radar::RadarState::default();
        self.radar_bubbles = radar::bubbles::BubbleState::default();
        self.review_comments.reset_view();
        self.comment_input.update(cx, |input, cx| input.clear(cx));
        self.context_paths.clear();
        self.project_name = project_name;
        self.branch_name = branch_name;
        self.commit_id = commit_id;
        self.repository_root = repository_root;
        self.branch_statuses.clear();
        self.refresh_branch_status();
        self.reference = reference;
        self.repository_files =
            load_repository_file_paths(&self.repository_root, &self.model.files);
        self.active_file = None;
        self.active_file_load = None;
        self.sidebar_width = px(272.);
        self.annotation_width = px(300.);
        self.outline_width = px(280.);
        self.diff_left_width = px(520.);
        self.diff_view = DiffView::Split;
        self.show_sidebar = true;
        self.show_annotations = false;
        self.show_outline = false;
        self.file_view = FileView::CompactTree;
        self.show_only_changes = true;
        self.hide_deleted_entries = false;
        self.collapsed_directories.clear();
        self.change_type_filters.clear();
        self.file_filters.clear();
        self.file_filter_matchers.clear();
        self.file_filter_input
            .update(cx, |input, cx| input.clear(cx));
        self.file_filter_error = None;
        self.show_file_filter = false;
        self.show_file_view_menu = false;
        self.show_branch_menu = false;
        self.show_settings_menu = false;
        self.selected_change = 0;
        self.collapsed_unchanged_sections.clear();
        self.hide_unchanged_sections = false;
        self.diff_list_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        self.model_revision = self.model_revision.wrapping_add(1);
    }

    fn switch_to_repository(
        &mut self,
        selected_path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repository_root = match resolve_repository_root(selected_path) {
            Ok(root) => root,
            Err(error) => {
                self.show_repository_error(error.to_string(), window, cx);
                return;
            }
        };
        let backend = match GitBackend::new(&repository_root) {
            Ok(backend) => backend,
            Err(error) => {
                self.show_repository_error(error.to_string(), window, cx);
                return;
            }
        };
        let options = DiffOptions {
            reference: None,
            pr: None,
            detect_pr: false,
            file: None,
            watch: false,
            theme: None,
            stacked: false,
            focus: None,
            origin: None,
            wrap: false,
        };
        let mut files = load_file_summaries(&options, &backend, &repository_root);
        sort_files_alphabetically(&mut files);
        let model = ReviewModel::new(files, None);
        let (watcher, watch_events, git_dir) = match setup_workspace_watcher(&repository_root) {
            Ok(watcher) => watcher,
            Err(error) => {
                self.show_repository_error(error.to_string(), window, cx);
                return;
            }
        };
        let branch_name = backend
            .get_current_branch()
            .ok()
            .flatten()
            .unwrap_or_else(|| "detached HEAD".to_string());
        let commit_id = review_commit_id(None, &backend);

        self.persist_project_settings();
        let _ = self.lsp_sender.send(LspCommand::Stop);
        let (lsp_sender, lsp_events) = lsp::spawn(repository_root.clone());
        self.lsp_sender = lsp_sender;
        self.lsp_events = lsp_events;
        self.lsp_status = LspStatus::Idle;
        self.lsp_symbols.clear();
        self.lsp_references.clear();
        self.show_lsp_menu = false;

        self.reset_workspace(
            model,
            project_name_for(&repository_root),
            branch_name,
            commit_id,
            repository_root.clone(),
            None,
            cx,
        );
        self.restore_project_settings(None, cx);
        self.rebuild_active_file();
        self.refresh_radar();
        self._workspace_watcher = watcher;
        self.workspace_watch_events = watch_events;
        self.workspace_git_dir = git_dir;
        self.show_project_menu = false;
        self.show_branch_menu = false;
        let _ = self.recent_repositories.record(&repository_root);
        window.set_window_title(&self.window_title());
        self.schedule_working_copy_counts();
        self.schedule_semantic_analysis();
        self.request_lsp_for_active_file();
        cx.notify();
    }

    fn show_repository_error(&self, detail: String, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window.prompt(
            PromptLevel::Warning,
            "Unable to open repository",
            Some(&detail),
            &["OK"],
            cx,
        );
    }

    fn close_menus(&mut self) -> bool {
        let was_open = self.review_comments.context_menu.is_some()
            || self.show_file_view_menu
            || self.show_project_menu
            || self.show_branch_menu
            || self.show_settings_menu
            || self.show_lsp_menu;
        self.review_comments.context_menu = None;
        self.show_file_view_menu = false;
        self.show_project_menu = false;
        self.show_branch_menu = false;
        self.show_settings_menu = false;
        self.show_lsp_menu = false;
        was_open
    }

    fn has_open_menu(&self) -> bool {
        self.review_comments.context_menu.is_some()
            || self.show_file_view_menu
            || self.show_project_menu
            || self.show_branch_menu
            || self.show_settings_menu
            || self.show_lsp_menu
    }

    fn dismiss_menus(&mut self, cx: &mut Context<Self>) {
        if self.close_menus() {
            cx.notify();
        }
    }

    fn toggle_project_menu(&mut self, cx: &mut Context<Self>) {
        let should_show = !self.show_project_menu;
        self.close_menus();
        self.show_project_menu = should_show;
        cx.notify();
    }

    fn toggle_branch_menu(&mut self, cx: &mut Context<Self>) {
        let should_show = !self.show_branch_menu;
        self.close_menus();
        self.show_branch_menu = should_show;
        cx.notify();
    }

    fn toggle_settings_menu(&mut self, cx: &mut Context<Self>) {
        let should_show = !self.show_settings_menu;
        self.close_menus();
        self.show_settings_menu = should_show;
        cx.notify();
    }

    fn set_theme(&mut self, theme: Theme, window: &mut Window, cx: &mut Context<Self>) {
        self.user_profile.theme = theme;
        theme.activate();
        if let Err(error) = self.user_profile.save() {
            let _ = window.prompt(
                PromptLevel::Warning,
                "Unable to save appearance",
                Some(&error.to_string()),
                &["OK"],
                cx,
            );
        }
        self.show_settings_menu = false;
        cx.notify();
    }

    fn switch_to_branch(
        &mut self,
        branch_name: &str,
        branch_type: BranchType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if branch_type == BranchType::Local && branch_name == self.branch_name {
            self.show_branch_menu = false;
            cx.notify();
            return;
        }

        let mut command = Command::new("git");
        command.current_dir(&self.repository_root).arg("switch");
        if branch_type == BranchType::Remote {
            let local_name = branch_name.split_once('/').map(|(_, name)| name);
            let local_exists = local_name.is_some_and(|name| {
                Repository::discover(&self.repository_root)
                    .ok()
                    .is_some_and(|repository| {
                        repository.find_branch(name, BranchType::Local).is_ok()
                    })
            });
            if local_exists {
                command.arg("--").arg(local_name.unwrap_or(branch_name));
            } else {
                command.arg("--track").arg(branch_name);
            }
        } else {
            command.arg("--").arg(branch_name);
        }

        match command.output() {
            Ok(output) if output.status.success() => {
                self.show_branch_menu = false;
                // Branch selection always returns the workspace to its working-copy review.
                self.reference = None;
                self.reload_repository(window, cx);
            }
            Ok(output) => {
                let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
                self.show_branch_error(
                    if detail.is_empty() {
                        format!("Git could not switch to {branch_name}.")
                    } else {
                        detail
                    },
                    window,
                    cx,
                );
            }
            Err(error) => self.show_branch_error(error.to_string(), window, cx),
        }
    }

    fn show_branch_error(&self, detail: String, window: &mut Window, cx: &mut Context<Self>) {
        let _ = window.prompt(
            PromptLevel::Warning,
            "Unable to switch branch",
            Some(&detail),
            &["OK"],
            cx,
        );
    }

    fn clear_recent_repositories(&mut self, cx: &mut Context<Self>) {
        let _ = self.recent_repositories.clear();
        cx.notify();
    }

    fn open_repository_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_project_menu = false;
        let paths_receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Repository".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths_receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.switch_to_repository(&path, window, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn open_repository_action(
        &mut self,
        _: &OpenRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_repository_picker(window, cx);
    }

    fn dismiss_menus_action(
        &mut self,
        _: &DismissMenus,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_menus(cx);
    }

    fn selected_file(&self) -> Option<&model::NativeFile> {
        self.is_file_selected(self.model.selected)
            .then_some(())
            .and_then(|_| self.active_file.as_ref())
    }

    fn is_file_selected(&self, index: usize) -> bool {
        self.active_log.is_none()
            && !self.radar.active
            && self.model.selected == index
            && self.tabs.iter().any(|tab| tab.file_index == index)
    }

    fn choose_file(&mut self, index: usize, pin_tab: bool, cx: &mut Context<Self>) {
        let changed_file = !self.is_file_selected(index);
        self.active_log = None;
        self.radar.active = false;
        if let Some(tab_index) = self.tabs.iter().position(|tab| tab.file_index == index) {
            self.model.selected = self.tabs[tab_index].file_index;
        } else if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.is_preview) {
            tab.file_index = index;
            self.model.selected = index;
        } else {
            self.tabs.push(ReviewTab {
                file_index: index,
                is_preview: true,
            });
            self.model.selected = index;
        }
        if pin_tab {
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.file_index == index) {
                tab.is_preview = false;
            }
        }
        if changed_file {
            self.unified_diff_scroll.scroll_to_item_strict(0, ScrollStrategy::Top);
            self.selected_change = 0;
            self.diff_list_scroll
                .scroll_to_item_strict(0, ScrollStrategy::Top);
        }
        self.rebuild_active_file();
        self.request_lsp_for_active_file();
        self.persist_project_settings();
        cx.notify();
    }

    fn choose_sidebar_file(&mut self, path: String, pin_tab: bool, cx: &mut Context<Self>) {
        let Some(index) = self.ensure_workspace_file(&path) else {
            return;
        };
        self.choose_file(index, pin_tab, cx);
    }

    fn choose_review_item(&mut self, path: &str, line: Option<usize>, cx: &mut Context<Self>) {
        let Some(index) = self.model.files.iter().position(|file| file.path == path) else {
            return;
        };
        self.choose_file(index, true, cx);
        let Some(line) = line else {
            return;
        };
        let Some(file) = self.selected_file() else {
            return;
        };
        if let Some(source_row) = file
            .rows
            .iter()
            .position(|row| row.new_number == Some(line) || row.old_number == Some(line))
        {
            if let Some(display_row) =
                display_rows_for_file(file, &self.collapsed_unchanged_sections, self.lsp_symbols.get(&file.path).map(Vec::as_slice).unwrap_or(&[]))
                    .iter()
                    .position(|row| {
                        matches!(row, DiffDisplayRow::Code { source_row: candidate } if *candidate == source_row)
                    })
            {
                self.diff_list_scroll
                    .scroll_to_item_strict(display_row, ScrollStrategy::Center);
            }
        }
        cx.notify();
    }

    fn jump_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let Some(file) = self.selected_file() else {
            return;
        };
        let Some(source_row) = file
            .rows
            .iter()
            .position(|row| row.new_number == Some(line) || row.old_number == Some(line))
        else {
            return;
        };
        if let Some(display_row) = display_rows_for_file(file, &self.collapsed_unchanged_sections, self.lsp_symbols.get(&file.path).map(Vec::as_slice).unwrap_or(&[]))
            .iter()
            .position(|row| {
                matches!(row, DiffDisplayRow::Code { source_row: candidate } if *candidate == source_row)
            })
        {
            self.diff_list_scroll
                .scroll_to_item_strict(display_row, ScrollStrategy::Center);
            cx.notify();
        }
    }

    fn toggle_lsp_menu(&mut self, cx: &mut Context<Self>) {
        let should_show = !self.show_lsp_menu;
        self.close_menus();
        self.show_lsp_menu = should_show;
        cx.notify();
    }

    fn restart_lsp(&mut self, cx: &mut Context<Self>) {
        self.lsp_status = LspStatus::Starting;
        self.push_lsp_log("Restarting TypeScript language server.".into());
        let _ = self.lsp_sender.send(LspCommand::Restart);
        self.request_lsp_for_active_file();
        cx.notify();
    }

    fn stop_lsp(&mut self, cx: &mut Context<Self>) {
        let _ = self.lsp_sender.send(LspCommand::Stop);
        self.push_lsp_log("Stopping TypeScript language server.".into());
        self.show_lsp_menu = false;
        cx.notify();
    }

    fn install_available_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(release) = self.available_update.as_ref() else {
            return;
        };
        match updater::install(release) {
            Ok(()) => cx.quit(),
            Err(error) => {
                let _ = window.prompt(
                    PromptLevel::Warning,
                    "Unable to install update",
                    Some(&error),
                    &["OK"],
                    cx,
                );
            }
        }
    }

    fn toggle_outline(&mut self, cx: &mut Context<Self>) {
        self.show_outline = !self.show_outline;
        if self.show_outline {
            self.show_lsp_menu = false;
        }
        self.persist_project_settings();
        cx.notify();
    }

    fn filtered_file_indices(&self) -> Vec<usize> {
        self.model
            .files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| {
                (!self.context_paths.contains(&file.path)
                    && !is_path_filtered(&file.path, &self.file_filter_matchers))
                .then_some(index)
            })
            .collect()
    }

    fn filtered_change_file_indices(&self) -> Vec<usize> {
        self.filtered_file_indices()
            .into_iter()
            .filter(|index| {
                self.model
                    .files
                    .get(*index)
                    .is_some_and(|file| file_matches_change_types(file, &self.change_type_filters))
            })
            .collect()
    }

    fn sidebar_file_entries(&self) -> Vec<SidebarFileEntry> {
        if self.show_only_changes {
            return self
                .filtered_change_file_indices()
                .into_iter()
                .filter(|index| {
                    !self.hide_deleted_entries
                        || self.model.files[*index].status
                            != crate::command::diff::types::FileStatus::Deleted
                })
                .filter_map(|index| {
                    self.model.files.get(index).map(|file| SidebarFileEntry {
                        path: file.path.clone(),
                        model_index: Some(index),
                        is_changed: true,
                    })
                })
                .collect();
        }

        let model_indices = self
            .model
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| (file.path.as_str(), index))
            .collect::<HashMap<_, _>>();
        let changed_indices = model_indices
            .iter()
            .filter(|(path, _)| !self.context_paths.contains::<str>(*path))
            .map(|(path, index)| (*path, *index))
            .collect::<HashMap<_, _>>();

        self.repository_files
            .iter()
            .filter(|path| !is_path_filtered(path, &self.file_filter_matchers))
            .filter_map(|path| {
                let changed_index = changed_indices.get(path.as_str()).copied();
                if self.hide_deleted_entries
                    && changed_index.is_some_and(|index| {
                        self.model.files[index].status
                            == crate::command::diff::types::FileStatus::Deleted
                    })
                {
                    return None;
                }
                if !self.change_type_filters.is_empty()
                    && !changed_index.is_some_and(|index| {
                        file_matches_change_types(
                            &self.model.files[index],
                            &self.change_type_filters,
                        )
                    })
                {
                    return None;
                }
                Some(SidebarFileEntry {
                    path: path.clone(),
                    model_index: model_indices.get(path.as_str()).copied(),
                    is_changed: changed_index.is_some(),
                })
            })
            .collect()
    }

    fn filtered_out_file_indices(&self) -> Vec<usize> {
        let visible = self.sidebar_file_entries().into_iter().map(|entry| entry.path).collect::<HashSet<_>>();
        self.model.files.iter().enumerate()
            .filter_map(|(index, file)| (!visible.contains(&file.path) && !self.context_paths.contains(&file.path)).then_some(index))
            .collect()
    }

    fn toggle_file_reviewed(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.reviewed_files.insert(path.to_string()) {
            self.reviewed_files.remove(path);
        }
        self.persist_project_settings();
        cx.notify();
    }

    fn toggle_all_files_reviewed(&mut self, cx: &mut Context<Self>) {
        let paths = self.model.files.iter()
            .filter(|file| !self.context_paths.contains(&file.path))
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let all_reviewed = !paths.is_empty() && paths.iter().all(|path| self.reviewed_files.contains(path));
        for path in paths {
            if all_reviewed { self.reviewed_files.remove(&path); }
            else { self.reviewed_files.insert(path); }
        }
        self.persist_project_settings();
        cx.notify();
    }

    fn refresh_radar(&mut self) {
        if self.radar.open {
            self.radar.use_bubbles = true;
        }
        self.radar.refresh(
            self.repository_root.clone(),
            self.reference.clone(),
            self.file_filters.clone(),
        );
        if self.radar.open {
            self.refresh_radar_bubbles();
        }
    }

    fn refresh_radar_bubbles(&mut self) {
        use radar::bubbles::FileInput;
        let changes = self.model.files.iter().map(|file| {
            let change = match file.status {
                crate::command::diff::types::FileStatus::Added => radar::Change::Added,
                crate::command::diff::types::FileStatus::Deleted => radar::Change::Removed,
                _ => radar::Change::Modified,
            };
            (file.path.as_str(), (change, file.additions + file.deletions))
        }).collect::<HashMap<_, _>>();
        let files = self.repository_files.iter().filter(|path| {
            !self.file_filter_matchers.iter().any(|filter| filter.is_match(path))
        }).map(|path| {
            let (change, churn) = changes.get(path.as_str()).copied().unwrap_or((radar::Change::Unchanged, 0));
            FileInput { path: path.clone(), change, churn }
        }).collect();
        self.radar_bubbles.refresh(self.repository_root.clone(), files, self.radar.dependent_counts());
    }

    fn hidden_file_count(&self) -> usize {
        if self.show_only_changes {
            self.model
                .files
                .iter()
                .filter(|f| !self.context_paths.contains(&f.path))
                .count()
                .saturating_sub(self.filtered_file_indices().len())
        } else {
            self.repository_files
                .len()
                .saturating_sub(self.sidebar_file_entries().len())
        }
    }

    fn toggle_file_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_file_view_menu = false;
        self.show_file_filter = !self.show_file_filter;
        self.file_filter_error = None;
        if self.show_file_filter {
            let focus_handle = self.file_filter_input.read(cx).focus_handle().clone();
            window.focus(&focus_handle);
        }
        self.persist_project_settings();
        cx.notify();
    }

    fn add_file_filter(&mut self, pattern: &str, cx: &mut Context<Self>) {
        if let Ok(glob) = globset::Glob::new(pattern) {
            let pattern = pattern.to_string();
            if !self.file_filters.contains(&pattern) {
                self.file_filter_matchers.push(glob.compile_matcher());
                self.file_filters.push(pattern);
            }
        }
        self.file_filter_error = None;
        self.refresh_radar();
        self.persist_project_settings();
        cx.notify();
    }

    fn remove_file_filter(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.file_filters.len() {
            self.file_filters.remove(index);
            self.file_filter_matchers.remove(index);
            self.file_filter_error = None;
            self.refresh_radar();
            self.persist_project_settings();
            cx.notify();
        }
    }

    fn clear_file_filters(&mut self, cx: &mut Context<Self>) {
        self.file_filters.clear();
        self.file_filter_matchers.clear();
        self.file_filter_error = None;
        self.refresh_radar();
        self.persist_project_settings();
        cx.notify();
    }

    fn scroll_to_selected_change(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.selected_file() else {
            return;
        };
        let change_rows = change_start_rows(&file.rows);
        let Some(row) = change_rows.get(self.selected_change) else {
            self.selected_change = 0;
            return;
        };
        let display_row = display_rows_for_file(file, &self.collapsed_unchanged_sections, self.lsp_symbols.get(&file.path).map(Vec::as_slice).unwrap_or(&[]))
            .iter()
            .position(|display_row| {
                matches!(display_row, DiffDisplayRow::Code { source_row } if source_row == row)
            })
            // Changed source rows are never collapsed. Keeping this fallback
            // makes navigation resilient if the display model changes later.
            .unwrap_or(*row);
        self.diff_list_scroll
            .scroll_to_item_strict(display_row, ScrollStrategy::Top);
        cx.notify();
    }

    fn select_previous_file_change(&mut self, cx: &mut Context<Self>) {
        if self.selected_change > 0 {
            self.selected_change -= 1;
            self.scroll_to_selected_change(cx);
            self.persist_project_settings();
        }
    }

    fn select_next_file_change(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.selected_file() else {
            return;
        };
        if self.selected_change + 1 < change_start_rows(&file.rows).len() {
            self.selected_change += 1;
            self.scroll_to_selected_change(cx);
            self.persist_project_settings();
        }
    }

    fn toggle_all_unchanged_sections(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.selected_file() else {
            return;
        };
        if logical_unchanged_sections(file, self.lsp_symbols.get(&file.path).map(Vec::as_slice).unwrap_or(&[])).is_empty() {
            return;
        }
        let path = file.path.clone();
        let sections = logical_unchanged_sections(file, self.lsp_symbols.get(&path).map(Vec::as_slice).unwrap_or(&[]));
        let all_collapsed = sections.iter().all(|section| self.collapsed_unchanged_sections.contains(section));
        self.hide_unchanged_sections = !all_collapsed;
        if all_collapsed {
            self.collapsed_unchanged_sections.retain(|section| section.file_path != path);
        } else {
            self.collapsed_unchanged_sections.extend(sections);
        }

        self.model_revision = self.model_revision.wrapping_add(1);
        self.persist_project_settings();
        cx.notify();
    }

    fn toggle_diff_view(&mut self, cx: &mut Context<Self>) {
        self.diff_view = match self.diff_view {
            DiffView::Split => DiffView::Unified,
            DiffView::Unified => DiffView::Split,
        };
        self.persist_project_settings();
        cx.notify();
    }

    fn toggle_unchanged_section(&mut self, section: UnchangedSection, cx: &mut Context<Self>) {
        if !self.collapsed_unchanged_sections.remove(&section) {
            self.collapsed_unchanged_sections.insert(section);
        }
        self.model_revision = self.model_revision.wrapping_add(1);
        self.persist_project_settings();
        cx.notify();
    }

    fn activate_tab(&mut self, tab_index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(tab_index) else {
            return;
        };
        self.model.selected = tab.file_index;
        self.active_log = None;
        self.radar.active = false;
        // A click on an italic preview tab is the explicit "keep" action.
        tab.is_preview = false;
        self.rebuild_active_file();
        self.persist_project_settings();
        cx.notify();
    }

    fn close_tab(&mut self, tab_index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(tab_index).copied() else {
            return;
        };
        let was_active = self.is_file_selected(tab.file_index);
        self.tabs.remove(tab_index);
        if was_active {
            if let Some(next_tab) = self.tabs.get(tab_index).or_else(|| self.tabs.last()) {
                self.model.selected = next_tab.file_index;
            }
        }
        self.rebuild_active_file();
        self.persist_project_settings();
        cx.notify();
    }

    fn open_log_tab(&mut self, log: LogTab, cx: &mut Context<Self>) {
        if !self.log_tabs.contains(&log) {
            self.log_tabs.push(log);
        }
        self.active_log = Some(log);
        self.radar.active = false;
        self.review_comments.active = false;
        self.show_lsp_menu = false;
        self.rebuild_active_file();
        cx.notify();
    }

    fn activate_log_tab(&mut self, log: LogTab, cx: &mut Context<Self>) {
        self.active_log = Some(log);
        self.radar.active = false;
        self.review_comments.active = false;
        self.rebuild_active_file();
        cx.notify();
    }

    fn close_log_tab(&mut self, log: LogTab, cx: &mut Context<Self>) {
        self.log_tabs.retain(|candidate| *candidate != log);
        if self.active_log == Some(log) {
            self.active_log = None;
            self.rebuild_active_file();
        }
        cx.notify();
    }

    fn clear_log_entries(&mut self, log: LogTab, cx: &mut Context<Self>) {
        match log {
            LogTab::Lsp => self.lsp_logs.clear(),
        }
        cx.notify();
    }

    fn push_lsp_log(&mut self, message: String) {
        if self.lsp_logs.last() != Some(&message) {
            self.lsp_logs.push(message);
        }
    }

    fn add_annotation(&mut self, cx: &mut Context<Self>) {
        self.model.add_annotation();
        let _ = self.model.persist_annotations();
        cx.notify();
    }

    fn start_sidebar_drag(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag = Some(DragKind::Sidebar {
            start: event.position.x,
            width: self.sidebar_width,
        });
        cx.notify();
    }

    fn start_filtered_panel_drag(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag = Some(DragKind::FilteredPanel { start: event.position.y, height: self.filtered_panel_height });
        cx.notify();
    }

    fn move_sidebar_drag(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(DragKind::Sidebar { start, width }) = self.drag {
            self.sidebar_width = constrain_sidebar_width(
                width + (event.position.x - start),
                window.viewport_size().width,
            );
            cx.notify();
        }
    }

    fn start_annotation_drag(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag = Some(DragKind::Annotations {
            start: event.position.x,
            width: self.annotation_width,
        });
        cx.notify();
    }

    fn start_outline_drag(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag = Some(DragKind::Outline {
            start: event.position.x,
            width: self.outline_width,
        });
        cx.notify();
    }

    fn start_diff_drag(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.drag = Some(DragKind::Diff {
            start: event.position.x,
            width: self.diff_left_width,
        });
        cx.notify();
    }

    fn start_left_code_scroll(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag = Some(DragKind::LeftCodeScroll {
            start: event.position.x,
            offset: self.left_code_scroll.offset().x,
        });
        cx.notify();
    }

    fn start_right_code_scroll(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag = Some(DragKind::RightCodeScroll {
            start: event.position.x,
            offset: self.right_code_scroll.offset().x,
        });
        cx.notify();
    }

    fn drag_code_scroll(
        handle: &ScrollHandle,
        start: Pixels,
        offset: Pixels,
        position: Pixels,
        width: Pixels,
    ) {
        let max_offset = handle.max_offset().width;
        if max_offset <= px(0.) {
            return;
        }
        let track_width = (width - px(16.)).max(px(1.));
        let delta = (position - start) * (max_offset / track_width);
        let current = handle.offset();
        handle.set_offset(point(
            (offset - delta).clamp(-max_offset, px(0.)),
            current.y,
        ));
    }

    fn move_diff_drag(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(DragKind::Diff { start, width }) = self.drag {
            self.diff_left_width = (width + (event.position.x - start)).clamp(px(240.), px(1200.));
            cx.notify();
        }
    }

    fn move_annotation_drag(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(DragKind::Annotations { start, width }) = self.drag {
            self.annotation_width = (width - (event.position.x - start)).clamp(px(220.), px(480.));
            cx.notify();
        }
    }

    fn move_outline_drag(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(DragKind::Outline { start, width }) = self.drag {
            self.outline_width = (width - (event.position.x - start)).clamp(px(220.), px(480.));
            cx.notify();
        }
    }

    fn stop_drag(&mut self, _: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_comment_selection(window, cx);
        let persisted_drag = matches!(
            self.drag,
            Some(
                DragKind::Sidebar { .. }
                    | DragKind::FilteredPanel { .. }
                    | DragKind::Annotations { .. }
                    | DragKind::Outline { .. }
                    | DragKind::Diff { .. }
            )
        );
        self.drag = None;
        if persisted_drag {
            self.persist_project_settings();
        }
        cx.notify();
    }

    fn track_drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !event.dragging() {
            return;
        }
        match self.drag {
            Some(DragKind::FilteredPanel { start, height }) => {
                self.filtered_panel_height = (height - (event.position.y - start)).clamp(px(80.), (window.viewport_size().height - px(200.)).max(px(80.)));
            }
            Some(DragKind::Sidebar { start, width }) => {
                self.sidebar_width = constrain_sidebar_width(
                    width + (event.position.x - start),
                    window.viewport_size().width,
                );
            }
            Some(DragKind::Annotations { start, width }) => {
                self.annotation_width =
                    (width - (event.position.x - start)).clamp(px(220.), px(480.));
            }
            Some(DragKind::Outline { start, width }) => {
                self.outline_width = (width - (event.position.x - start)).clamp(px(220.), px(480.));
            }
            Some(DragKind::Diff { start, width }) => {
                self.diff_left_width =
                    (width + (event.position.x - start)).clamp(px(240.), px(1200.));
            }
            Some(DragKind::LeftCodeScroll { start, offset }) => {
                Self::drag_code_scroll(
                    &self.left_code_scroll,
                    start,
                    offset,
                    event.position.x,
                    self.diff_left_width,
                );
            }
            Some(DragKind::RightCodeScroll { start, offset }) => {
                Self::drag_code_scroll(
                    &self.right_code_scroll,
                    start,
                    offset,
                    event.position.x,
                    self.right_code_scroll.bounds().size.width + px(66.),
                );
            }
            None => return,
        }
        cx.notify();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.show_sidebar = !self.show_sidebar;
        self.persist_project_settings();
        cx.notify();
    }

    fn set_file_view(&mut self, file_view: FileView, cx: &mut Context<Self>) {
        self.file_view = file_view;
        self.show_file_view_menu = false;
        self.persist_project_settings();
        cx.notify();
    }

    fn toggle_file_view_menu(&mut self, cx: &mut Context<Self>) {
        let should_show = !self.show_file_view_menu;
        self.close_menus();
        self.show_file_view_menu = should_show;
        cx.notify();
    }

    fn toggle_show_only_changes(&mut self, cx: &mut Context<Self>) {
        self.show_only_changes = !self.show_only_changes;
        self.show_file_view_menu = false;
        self.persist_project_settings();
        cx.notify();
    }

    fn toggle_hide_deleted_entries(&mut self, cx: &mut Context<Self>) {
        self.hide_deleted_entries = !self.hide_deleted_entries;
        self.show_file_view_menu = false;
        self.persist_project_settings();
        cx.notify();
    }

    fn toggle_change_type_filter(
        &mut self,
        family: view::ReviewChangeFamily,
        cx: &mut Context<Self>,
    ) {
        if !self.change_type_filters.insert(family) {
            self.change_type_filters.remove(&family);
        }
        self.persist_project_settings();
        cx.notify();
    }

    fn clear_change_type_filters(&mut self, cx: &mut Context<Self>) {
        self.change_type_filters.clear();
        self.persist_project_settings();
        cx.notify();
    }

    fn toggle_directory(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.collapsed_directories.insert(path.clone()) {
            self.collapsed_directories.remove(&path);
        }
        self.persist_project_settings();
        cx.notify();
    }
}

impl Render for ReviewWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.radar.active {
            if self.radar_bubbles.hovered.is_some() {
                self.radar_bubbles.clear_hover_emphasis();
            }
            self.radar_bubbles.view_menu_open = false;
        }
        self.sidebar_width =
            constrain_sidebar_width(self.sidebar_width, window.viewport_size().width);
        let content = div()
            .flex()
            .flex_1()
            .overflow_hidden()
            .when(self.show_sidebar && !self.radar.active, |this| {
                this.child(self.sidebar(window, cx))
                    .child(self.sidebar_resizer(cx))
            })
            .child(self.diff_canvas(cx))
            .when(
                self.show_annotations && !self.radar.active && !self.review_comments.active,
                |this| {
                    this.child(self.annotations_resizer(cx))
                        .child(self.annotations(cx))
                },
            )
            .when(
                self.show_outline && !self.radar.active && !self.review_comments.active,
                |this| {
                    this.child(self.outline_resizer(cx))
                        .child(self.outline_panel(cx))
                },
            );

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family(".SystemUIFont")
            .text_size(px(12.))
            // Keep tracking on the full workspace while dragging. The divider
            // itself only starts the gesture; movement must continue after the
            // pointer leaves its six-pixel hit target.
            .on_mouse_move(cx.listener(Self::track_drag))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_drag))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::stop_drag))
            .on_action(cx.listener(Self::open_repository_action))
            .on_action(cx.listener(Self::dismiss_menus_action))
            .on_action(|_: &MinimizeWindow, window, _| window.minimize_window())
            .child(self.top_bar(cx))
            .child(content)
            .child(self.bottom_bar(cx))
            .when(self.review_comments.active, |this| {
                this.child(self.comments_canvas(cx))
            })
            .when(self.has_open_menu(), |this| {
                this.child(
                    div()
                        .id("menu-dismiss-layer")
                        .occlude()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_menus(cx))),
                )
            })
            .when(self.show_file_view_menu, |this| {
                this.child(self.file_view_menu(cx))
            })
            .when(self.show_lsp_menu, |this| this.child(self.lsp_menu(cx)))
            .when(self.show_project_menu, |this| {
                this.child(self.project_menu(cx))
            })
            .when(self.show_branch_menu, |this| {
                this.child(self.branch_menu(cx))
            })
            .when(self.show_settings_menu, |this| {
                this.child(self.settings_menu(cx))
            })
            .when(self.review_comments.context_menu.is_some(), |this| {
                this.child(self.comment_context_menu(cx))
            })
            .when(self.review_comments.draft.is_some(), |this| {
                this.child(self.comment_dialog(cx))
            })
    }
}

impl ReviewWorkspace {
    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(38.))
            .w_full()
            .relative()
            .pl(px(76.))
            .pr(px(76.))
            .flex()
            .items_center()
            .gap(px(4.))
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x272d39))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("project-switcher")
                            .min_w(px(0.))
                            .max_w(px(200.))
                            .h(px(26.))
                            .px_2()
                            .pr(px(4.))
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|element| element.bg(rgb(0x343c4a)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_project_menu(cx)))
                            .child(
                                svg()
                                    .path("icons/folder_open.svg")
                                    .size(px(14.))
                                    .flex_none()
                                    .text_color(rgb(MUTED)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(self.project_name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .id("branch-switcher")
                            .min_w(px(0.))
                            .max_w(px(320.))
                            .h(px(26.))
                            .px_2()
                            .pl(px(4.))
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|element| element.bg(rgb(0x343c4a)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_branch_menu(cx)))
                            .child(
                                svg()
                                    .path("icons/git_branch.svg")
                                    .size(px(14.))
                                    .flex_none()
                                    .text_color(rgb(MUTED)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(0xb5beca))
                                    .truncate()
                                    .child(self.branch_name.clone()),
                            )
                            .when_some(
                                self.branch_statuses.get(&self.branch_name),
                                |this, status| {
                                    this.child(Self::branch_status_badge(
                                        "current-branch-status".into(),
                                        status,
                                    ))
                                },
                            ),
                    ),
            )
            .child(self.workspace_switch(cx))
            .child(div().flex_1().min_w(px(0.)))
            .when_some(self.available_update.as_ref(), |this, release| {
                let version = release.version.clone();
                this.child(
                    div()
                        .id("install-update")
                        .h(px(26.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(rgb(0x101720))
                        .bg(rgb(GREEN))
                        .hover(|element| element.bg(rgb(0x95d8b3)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.install_available_update(window, cx)
                        }))
                        .child(format!("Update {version}")),
                )
            })
            .child(
                div()
                    .id("settings-toggle")
                    .absolute()
                    .right(px(12.))
                    .size(px(26.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(rgb(if self.show_settings_menu { BLUE } else { MUTED }))
                    .when(self.show_settings_menu, |element| element.bg(rgb(0x35425a)))
                    .hover(|element| element.bg(rgb(0x343c4a)).text_color(rgb(TEXT)))
                    .tooltip(|_, cx| cx.new(|_| view::ReviewTooltip("Settings".into())).into())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_settings_menu(cx)))
                    .child(Self::file_view_settings_icon(if self.show_settings_menu {
                        BLUE
                    } else {
                        MUTED
                    })),
            )
    }

    fn workspace_switch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("workspace-switch")
            .h(px(26.))
            .p(px(2.))
            .flex_none()
            .flex()
            .items_center()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL))
            .child(self.workspace_switch_option("changes-mode", "Changes", !self.radar.active, cx))
            .child(self.workspace_switch_option("radar-mode", "Radar", self.radar.active, cx))
    }

    fn workspace_switch_option(
        &self,
        id: &'static str,
        label: &'static str,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(22.))
            .w(px(70.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor_pointer()
            .text_size(px(11.))
            .text_color(rgb(if active { TEXT } else { MUTED }))
            .when(active, |element| element.bg(rgb(0x465166)))
            .hover(|element| element.bg(rgb(0x3a4350)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if label == "Radar" {
                    this.open_radar(cx);
                } else {
                    this.show_changes(cx);
                }
            }))
            .child(label)
    }

    fn show_changes(&mut self, cx: &mut Context<Self>) {
        if self.radar.active {
            self.radar.active = false;
            self.rebuild_active_file();
            self.request_lsp_for_active_file();
            self.persist_project_settings();
            cx.notify();
        }
    }

    fn settings_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-menu")
            .absolute()
            .top(px(38.))
            .right(px(12.))
            .w(px(224.))
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x303744))
            .shadow_lg()
            .child(
                div()
                    .h(px(26.))
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("APPEARANCE"),
            )
            .child(self.theme_menu_row("dark-theme", "Dark", Theme::Dark, cx))
            .child(self.theme_menu_row("light-theme", "Light", Theme::Light, cx))
    }

    fn theme_menu_row(
        &self,
        id: &'static str,
        label: &'static str,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .mx_1()
            .h(px(28.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_sm()
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(rgb(TEXT))
            .hover(|element| element.bg(rgb(0x3b4658)))
            .on_click(cx.listener(move |this, _, window, cx| this.set_theme(theme, window, cx)))
            .child(
                div()
                    .w(px(16.))
                    .text_color(rgb(BLUE))
                    .child(if self.user_profile.theme == theme { "✓" } else { "" }),
            )
            .child(label)
    }

    fn branch_status_badge(id: String, status: &branch_status::BranchStatus) -> impl IntoElement {
        let color = match status {
            branch_status::BranchStatus::Tracking { behind, .. } if *behind > 0 => YELLOW,
            branch_status::BranchStatus::Tracking { ahead, .. } if *ahead > 0 => BLUE,
            _ => MUTED,
        };
        let tooltip = status.tooltip();
        div()
            .id(gpui::SharedString::from(id))
            .flex_none()
            .ml_1()
            .text_size(px(11.))
            .text_color(rgb(color))
            .whitespace_nowrap()
            .tooltip(move |_, cx| cx.new(|_| view::ReviewTooltip(tooltip.clone())).into())
            .child(status.label())
    }

    fn branch_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (mut local_branches, remote_branches) = repository_branches(&self.repository_root);
        local_branches.sort_by_key(|name| (name != &self.branch_name, name.to_lowercase()));

        let local_rows = local_branches
            .into_iter()
            .enumerate()
            .map(|(index, branch_name)| {
                let status = self.branch_statuses.get(&branch_name);
                let is_current = branch_name == self.branch_name;
                let selection = branch_name.clone();
                div()
                    .id(("local-branch", index))
                    .h(px(26.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|element| element.bg(rgb(0x3b4658)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.switch_to_branch(&selection, BranchType::Local, window, cx)
                    }))
                    .child(
                        div()
                            .w(px(14.))
                            .h(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(is_current, |this| {
                                this.text_size(px(11.)).text_color(rgb(BLUE)).child("✓")
                            })
                            .when(!is_current, |this| {
                                this.child(
                                    svg()
                                        .path("icons/git_branch.svg")
                                        .size(px(14.))
                                        .text_color(rgb(MUTED)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(rgb(TEXT))
                            .truncate()
                            .child(branch_name),
                    )
                    .when_some(status, |this, status| {
                        this.child(Self::branch_status_badge(format!("branch-status-{index}"), status))
                    })
            })
            .collect::<Vec<_>>();

        let remote_rows = remote_branches
            .into_iter()
            .enumerate()
            .map(|(index, branch_name)| {
                let selection = branch_name.clone();
                div()
                    .id(("remote-branch", index))
                    .h(px(26.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|element| element.bg(rgb(0x3b4658)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.switch_to_branch(&selection, BranchType::Remote, window, cx)
                    }))
                    .child(
                        svg()
                            .path("icons/git_branch.svg")
                            .size(px(14.))
                            .flex_none()
                            .text_color(rgb(MUTED)),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(rgb(TEXT))
                            .truncate()
                            .child(branch_name),
                    )
            })
            .collect::<Vec<_>>();

        div()
            .id("branch-menu")
            .absolute()
            .top(px(38.))
            .left(px(250.))
            .w(px(360.))
            .max_h(px(520.))
            .overflow_y_scroll()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x303744))
            .shadow_lg()
            .child(
                div()
                    .h(px(22.))
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("LOCAL BRANCHES"),
            )
            .children(local_rows)
            .child(div().my_1().h(px(1.)).bg(rgb(BORDER)))
            .child(
                div()
                    .h(px(22.))
                    .px_3()
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("REMOTE BRANCHES"),
            )
            .children(remote_rows)
    }

    fn project_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let recent_rows = self
            .recent_repositories
            .paths()
            .iter()
            .filter(|path| path.exists())
            .enumerate()
            .map(|(index, path)| {
                let path = path.clone();
                let selection_path = path.clone();
                let current = path == self.repository_root;
                div()
                    .id(("recent-repository", index))
                    .h(px(42.))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|element| element.bg(rgb(0x3b4658)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.switch_to_repository(&selection_path, window, cx)
                    }))
                    .child(
                        div()
                            .w(px(14.))
                            .text_size(px(11.))
                            .text_color(rgb(BLUE))
                            .child(if current { "✓" } else { "" }),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(project_name_for(&path)),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED))
                                    .truncate()
                                    .child(abbreviated_path(&path)),
                            ),
                    )
            })
            .collect::<Vec<_>>();
        let has_recents = !recent_rows.is_empty();

        div()
            .id("project-menu")
            .absolute()
            .top(px(38.))
            .left(px(88.))
            .w(px(360.))
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x303744))
            .shadow_lg()
            .child(
                div()
                    .id("open-repository")
                    .h(px(30.))
                    .px_3()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(|element| element.bg(rgb(0x3b4658)))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_repository_picker(window, cx)),
                    )
                    .child(div().flex_1().text_size(px(12.)).child("Open Repository…"))
                    .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child("⌘O")),
            )
            .when(has_recents, |this| {
                this.child(div().my_1().h(px(1.)).bg(rgb(BORDER)))
                    .child(
                        div()
                            .h(px(22.))
                            .px_3()
                            .flex()
                            .items_center()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child("RECENT REPOSITORIES"),
                    )
                    .children(recent_rows)
                    .child(div().my_1().h(px(1.)).bg(rgb(BORDER)))
                    .child(
                        div()
                            .id("clear-recent-repositories")
                            .h(px(30.))
                            .px_3()
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .hover(|element| element.bg(rgb(0x3b4658)).text_color(rgb(TEXT)))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.clear_recent_repositories(cx)),
                            )
                            .child("Clear Recent Repositories"),
                    )
            })
    }

    fn sidebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible_file_count = self.sidebar_file_entries().len();
        div()
            .w(self.sidebar_width)
            .h_full()
            .flex_none()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .child(
                div()
                    .w_full()
                    .h(px(34.))
                    .px_3()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .text_size(px(11.))
                    .text_color(rgb(TEXT))
                    .child("FILES")
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .child(visible_file_count.to_string()),
                    )
                    .child(div().flex_1())
                    .child(self.review_all_files_button(cx))
                    .child(self.file_view_settings_button(cx)),
            )
            .child(
                div()
                    .id("file-scroll")
                    .flex_1()
                    .px_2()
                    .pr(px(6.))
                    .pt_1()
                    .overflow_scroll()
                    .child(self.file_list(cx)),
            )
            .child(
                div()
                    .id("filtered-files-resizer")
                    .h(px(5.))
                    .w_full()
                    .flex_none()
                    .cursor(CursorStyle::ResizeUpDown)
                    .flex()
                    .items_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::start_filtered_panel_drag),
                    )
                    .child(div().h(px(1.)).w_full().bg(rgb(BORDER))),
            )
            .child(self.filtered_files_panel(window, cx))
    }

    fn filtered_files_panel(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let indices = self.filtered_out_file_indices();
        let count = indices.len();
        let active_filters = self.file_filters.len() + self.change_type_filters.len();
        let filter_color = if active_filters > 0 { BLUE } else { MUTED };
        div()
            .id("filtered-files-panel")
            .h(self.filtered_panel_height)
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(PANEL))
            .child(div().h(px(28.)).px_3().flex().items_center()
                .text_size(px(11.)).text_color(rgb(MUTED))
                .child(format!("IGNORED · {count}"))
                .child(div().flex_1())
                .child(self.file_filter_toggle(false, filter_color, active_filters, cx)))
            .when(self.show_file_filter, |this| {
                this.child(self.file_filter_panel(window, self.hidden_file_count(), cx))
                    .child(self.change_type_filter_bar(cx))
            })
            .child(div().id("filtered-files-scroll").flex_1().px_2().pr(px(6.)).overflow_scroll()
                .when(count == 0, |this| this.child(div().px_1().pt_2().text_size(px(11.))
                    .text_color(rgb(MUTED)).child("No ignored files")))
                .children(indices.into_iter().map(|index| {
                    let file = &self.model.files[index];
                    self.file_row(SidebarFileEntry {
                        path: file.path.clone(),
                        model_index: Some(index),
                        is_changed: true,
                    }, 0, true, &[], cx)
                })))
    }

    fn file_filter_toggle(
        &self,
        show_counter: bool,
        filter_color: u32,
        active_filters: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(if show_counter { "file-filter-toggle" } else { "ignored-filter-toggle" })
            .h(px(22.))
            .min_w(px(24.))
            .px(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(3.))
            .rounded_sm()
            .cursor_pointer()
            .text_size(px(11.))
            .text_color(rgb(filter_color))
            .bg(if active_filters > 0 { rgb(0x263b5a) } else { rgb(PANEL) })
            .hover(|element| element.bg(rgb(0x3a4350)).text_color(rgb(TEXT)))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_file_filter(window, cx)))
            .tooltip(|_, cx| cx.new(|_| view::ReviewTooltip("Show or hide filters".into())).into())
            .child(Self::file_filter_icon(filter_color))
            .when(show_counter && active_filters > 0, |this| {
                this.child(div().min_w(px(10.)).text_center().text_size(px(10.))
                    .child(active_filters.to_string()))
            })
    }

    fn file_filter_panel(
        &self,
        window: &Window,
        hidden_files: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active_count = self.file_filters.len();
        let input_focus_handle = self.file_filter_input.read(cx).focus_handle().clone();
        let input_focused = input_focus_handle.is_focused(window);
        let status = if active_count == 0 {
            "No filters active".to_string()
        } else {
            format!("{} active · {} hidden", active_count, hidden_files)
        };
        let patterns =
            self.file_filters
                .iter()
                .enumerate()
                .map(|(index, pattern)| {
                    let pattern = pattern.clone();
                    div()
                        .id(("file-filter-pattern", index))
                        .h(px(22.))
                        .w_full()
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_1()
                        .rounded_sm()
                        .bg(rgb(0x263b5a))
                        .text_size(px(11.))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(rgb(TEXT))
                                .child(pattern),
                        )
                        .child(
                            div()
                                .id(("remove-file-filter", index))
                                .w(px(16.))
                                .h(px(16.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .cursor_pointer()
                                .text_color(rgb(MUTED))
                                .hover(|element| element.bg(rgb(0x465166)).text_color(rgb(TEXT)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_file_filter(index, cx)
                                }))
                                .child("×"),
                        )
                });

        div()
            .w_full()
            .px_2()
            .pb_2()
            .flex()
            .flex_col()
            .gap_1()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .h(px(18.))
                    .w_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child(status),
                    )
                    .when(active_count > 0, |this| {
                        this.child(
                            div()
                                .id("clear-file-filters")
                                .px_1()
                                .cursor_pointer()
                                .text_size(px(11.))
                                .text_color(rgb(BLUE))
                                .hover(|element| element.text_color(rgb(TEXT)))
                                .on_click(cx.listener(|this, _, _, cx| this.clear_file_filters(cx)))
                                .child("Clear"),
                        )
                    }),
            )
            .children(patterns)
            .child(
                div()
                    .id("file-filter-input")
                    .h(px(28.))
                    .w_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(if self.file_filter_error.is_some() {
                        RED
                    } else if input_focused {
                        BLUE
                    } else {
                        BORDER
                    }))
                    .bg(rgb(if input_focused { 0x2d3c51 } else { 0x242b36 }))
                    .cursor(CursorStyle::IBeam)
                    .track_focus(&input_focus_handle)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let focus_handle = this.file_filter_input.read(cx).focus_handle().clone();
                        window.focus(&focus_handle);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .min_w(px(0.))
                            .text_size(px(11.))
                            .child(self.file_filter_input.clone()),
                    )
                    .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child("↵")),
            )
            .children(
                self.file_filter_error
                    .as_ref()
                    .map(|error| div().text_size(px(11.)).text_color(rgb(RED)).child(error.clone())),
            )
    }

    fn file_view_settings_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active =
            self.show_file_view_menu || !self.show_only_changes || self.hide_deleted_entries;
        div()
            .id("file-view-settings-toggle")
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor_pointer()
            .text_color(rgb(if active { BLUE } else { MUTED }))
            .when(active, |element| element.bg(rgb(0x35425a)))
            .hover(|element| element.bg(rgb(0x3a4350)).text_color(rgb(TEXT)))
            .tooltip(|_, cx| {
                cx.new(|_| view::ReviewTooltip("File view settings".into()))
                    .into()
            })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_file_view_menu(cx)))
            .child(Self::file_view_settings_icon(if active {
                BLUE
            } else {
                MUTED
            }))
    }

    fn review_all_files_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let paths = self
            .model
            .files
            .iter()
            .filter(|file| !self.context_paths.contains(&file.path))
            .map(|file| &file.path)
            .collect::<Vec<_>>();
        let all_reviewed =
            !paths.is_empty() && paths.iter().all(|path| self.reviewed_files.contains(*path));
        div()
            .id("review-all-files")
            .size(px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor_pointer()
            .hover(|element| element.bg(rgb(0x3a4350)))
            .tooltip(move |_, cx| {
                cx.new(|_| {
                    view::ReviewTooltip(
                        if all_reviewed {
                            "Mark all files unreviewed"
                        } else {
                            "Mark all files reviewed"
                        }
                        .into(),
                    )
                })
                .into()
            })
            .on_click(cx.listener(|this, _, _, cx| this.toggle_all_files_reviewed(cx)))
            .child(Self::review_checkbox(all_reviewed))
    }

    fn file_view_menu_row(
        &self,
        id: &'static str,
        view: FileView,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.file_view == view;
        div()
            .id(id)
            .mx_1()
            .h(px(24.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_sm()
            .cursor_pointer()
            .text_size(px(11.))
            .text_color(rgb(TEXT))
            .hover(|element| element.bg(rgb(0x3b4350)))
            .on_click(cx.listener(move |this, _, _, cx| this.set_file_view(view, cx)))
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .text_color(rgb(BLUE))
                    .child(if active { "✓" } else { "" }),
            )
            .child(label)
    }

    fn file_view_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("file-view-settings-menu")
            .absolute()
            .top(px(72.))
            .left(self.sidebar_width - px(172.))
            .w(px(164.))
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x303641))
            .shadow_lg()
            .child(
                div()
                    .h(px(20.))
                    .px_2()
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("View"),
            )
            .child(self.file_view_menu_row("flat-file-view", FileView::Flat, "List", cx))
            .child(self.file_view_menu_row("tree-file-view", FileView::Tree, "Tree", cx))
            .child(self.file_view_menu_row(
                "compact-tree-file-view",
                FileView::CompactTree,
                "Compact",
                cx,
            ))
            .child(div().my(px(3.)).h(px(1.)).bg(rgb(BORDER)))
            .child(
                div()
                    .id("show-only-changes")
                    .mx_1()
                    .h(px(24.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(rgb(TEXT))
                    .hover(|element| element.bg(rgb(0x3b4350)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_show_only_changes(cx)))
                    .child(
                        div()
                            .w(px(16.))
                            .flex_none()
                            .text_color(rgb(BLUE))
                            .child(if self.show_only_changes { "✓" } else { "" }),
                    )
                    .child("Show only changes"),
            )
            .child(
                div()
                    .id("hide-deleted-entries")
                    .mx_1()
                    .h(px(24.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(rgb(TEXT))
                    .hover(|element| element.bg(rgb(0x3b4350)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_hide_deleted_entries(cx)))
                    .child(
                        div()
                            .w(px(16.))
                            .flex_none()
                            .text_color(rgb(BLUE))
                            .child(if self.hide_deleted_entries { "✓" } else { "" }),
                    )
                    .child("Hide deleted entries"),
            )
    }

    fn sidebar_resizer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("sidebar-resizer")
            .w(px(2.))
            .h_full()
            .flex_none()
            .cursor(CursorStyle::ResizeLeftRight)
            .bg(rgb(BORDER))
            .hover(|element| element.bg(rgb(BLUE)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::start_sidebar_drag))
            .on_mouse_move(cx.listener(Self::move_sidebar_drag))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_drag))
    }

    fn annotations_resizer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("annotations-resizer")
            .w(px(2.))
            .h_full()
            .cursor(CursorStyle::ResizeLeftRight)
            .bg(rgb(BORDER))
            .hover(|element| element.bg(rgb(BLUE)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::start_annotation_drag))
            .on_mouse_move(cx.listener(Self::move_annotation_drag))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_drag))
    }

    fn outline_resizer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("outline-resizer")
            .w(px(2.))
            .h_full()
            .flex_none()
            .cursor(CursorStyle::ResizeLeftRight)
            .bg(rgb(BORDER))
            .hover(|element| element.bg(rgb(BLUE)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::start_outline_drag))
            .on_mouse_move(cx.listener(Self::move_outline_drag))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_drag))
    }

    fn bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let lsp_ready = self.lsp_status.is_ready();
        let lsp_color = self.lsp_status_color();
        let has_outline = self
            .selected_file()
            .is_some_and(|file| is_typescript_path(&file.path));
        let outline_toggle_enabled = !self.radar.active && (has_outline || self.show_outline);
        div()
            .h(px(26.))
            .w_full()
            .px_2()
            .flex()
            .items_center()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL))
            .child(
                div()
                    .id("toggle-changes")
                    .w(px(28.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|element| element.bg(rgb(0x35425a)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
                    .child(Self::sidebar_toggle_icon(self.show_sidebar)),
            )
            .child(
                div()
                    .id("typescript-lsp-status")
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(rgb(if lsp_ready { BLUE } else { MUTED }))
                    .when(self.show_lsp_menu, |element| element.bg(rgb(0x35425a)))
                    .hover(|element| element.bg(rgb(0x35425a)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_lsp_menu(cx)))
                    .child(div().size(px(7.)).rounded_full().bg(rgb(lsp_color)))
                    .child("TypeScript"),
            )
            .child(div().flex_1())
            .child(self.comments_toggle(cx))
            .child(
                div()
                    .id("toggle-outline")
                    .h_full()
                    .w(px(32.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(2.))
                    .text_color(rgb(if outline_toggle_enabled {
                        TEXT
                    } else {
                        0x626b79
                    }))
                    .when(
                        self.show_outline && !self.radar.active && !self.review_comments.active,
                        |element| element.bg(rgb(0x35425a)),
                    )
                    .when(outline_toggle_enabled, |element| {
                        element
                            .cursor_pointer()
                            .hover(|element| element.bg(rgb(0x35425a)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_outline(cx)))
                    })
                    .child(
                        div()
                            .w(px(14.))
                            .h(px(1.))
                            .bg(rgb(if outline_toggle_enabled {
                                TEXT
                            } else {
                                0x626b79
                            })),
                    )
                    .child(
                        div()
                            .w(px(10.))
                            .h(px(1.))
                            .bg(rgb(if outline_toggle_enabled {
                                TEXT
                            } else {
                                0x626b79
                            })),
                    )
                    .child(
                        div()
                            .w(px(14.))
                            .h(px(1.))
                            .bg(rgb(if outline_toggle_enabled {
                                TEXT
                            } else {
                                0x626b79
                            })),
                    ),
            )
    }

    fn lsp_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.lsp_status.label();
        div()
            .id("typescript-lsp-menu")
            .absolute()
            .left(px(38.))
            .bottom(px(28.))
            .w(px(258.))
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x303641))
            .shadow_lg()
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("TypeScript language server"),
            )
            .child(
                div()
                    .mx_1()
                    .h(px(34.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_sm()
                    .bg(rgb(0x454c5b))
                    .child(
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(rgb(self.lsp_status_color())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(11.)).text_color(rgb(TEXT)).child(
                                match &self.lsp_status {
                                    LspStatus::Ready(name) => name.clone(),
                                    _ => "TypeScript LSP".to_string(),
                                },
                            ))
                            .child(div().text_size(px(9.)).text_color(rgb(MUTED)).child(status)),
                    ),
            )
            .child(div().my_1().h(px(1.)).bg(rgb(BORDER)))
            .child(
                div()
                    .id("open-lsp-logs")
                    .h(px(28.))
                    .px_2()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .hover(|element| element.bg(rgb(0x3b4350)))
                    .on_click(cx.listener(|this, _, _, cx| this.open_log_tab(LogTab::Lsp, cx)))
                    .child("Open Logs"),
            )
            .child(
                div()
                    .id("restart-typescript-lsp")
                    .h(px(28.))
                    .px_2()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .hover(|element| element.bg(rgb(0x3b4350)))
                    .on_click(cx.listener(|this, _, _, cx| this.restart_lsp(cx)))
                    .child("Restart Server"),
            )
            .child(
                div()
                    .id("stop-typescript-lsp")
                    .h(px(28.))
                    .px_2()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .hover(|element| element.bg(rgb(0x3b4350)))
                    .on_click(cx.listener(|this, _, _, cx| this.stop_lsp(cx)))
                    .child("Stop Server"),
            )
    }

    fn lsp_status_color(&self) -> u32 {
        match self.lsp_status {
            LspStatus::Failed(_) => RED,
            LspStatus::Starting => {
                if self.status_pulse_visible {
                    GREEN
                } else {
                    0x355848
                }
            }
            LspStatus::Ready(_) => GREEN,
            LspStatus::Idle | LspStatus::Stopped | LspStatus::Unavailable => YELLOW,
        }
    }

    fn outline_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self
            .selected_file()
            .map(|file| file.path.clone())
            .unwrap_or_default();
        let lsp_symbols = self.lsp_symbols.get(&path).cloned().unwrap_or_default();
        let using_lsp = !lsp_symbols.is_empty();
        let symbols = if using_lsp {
            lsp_symbols
        } else {
            self.selected_file()
                .map(|file| {
                    file.semantic
                        .symbols
                        .iter()
                        .map(|symbol| LspSymbol {
                            name: symbol.name.clone(),
                            kind: symbol.kind.label().to_string(),
                            line: symbol.line,
                            end_line: symbol.end_line,
                            depth: matches!(symbol.kind, semantic::SymbolKind::Method) as usize,
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let rows = symbols
            .into_iter()
            .enumerate()
            .map(|(index, symbol)| {
                let line = symbol.line;
                div()
                    .id(("outline-symbol", index))
                    .h(px(28.))
                    .pl(px(12. + symbol.depth as f32 * 14.))
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(|element| element.bg(rgb(0x3b4350)))
                    .on_click(cx.listener(move |this, _, _, cx| this.jump_to_line(line, cx)))
                    .child(
                        div()
                            .w(px(16.))
                            .text_center()
                            .text_color(rgb(BLUE))
                            .child("◇"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(rgb(TEXT))
                            .child(symbol.name),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(MUTED))
                            .child(format!("{} · L{}", symbol.kind, symbol.line)),
                    )
            })
            .collect::<Vec<_>>();
        let is_empty = rows.is_empty();
        div()
            .id("outline-panel")
            .w(self.outline_width)
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(rgb(PANEL))
            .child(
                div()
                    .h(px(44.))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(rgb(TEXT))
                                    .child("OUTLINE"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(9.))
                                    .text_color(rgb(MUTED))
                                    .child(path),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(rgb(if using_lsp { GREEN } else { BLUE }))
                            .child(if using_lsp { "LSP" } else { "AST" }),
                    ),
            )
            .child(
                div()
                    .id("outline-scroll")
                    .p_2()
                    .flex_1()
                    .overflow_scroll()
                    .flex()
                    .flex_col()
                    .when(is_empty, |this| {
                        this.child(
                            div()
                                .p_3()
                                .text_size(px(11.))
                                .text_color(rgb(MUTED))
                                .child("No TypeScript symbols found."),
                        )
                    })
                    .children(rows),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn file(path: &str) -> model::NativeFile {
        model::NativeFile {
            path: path.to_string(),
            status: crate::command::diff::types::FileStatus::Modified,
            additions: 0,
            deletions: 0,
            review_fingerprint: path.to_string(),
            rows: Arc::from(Vec::<NativeRow>::new()),
            max_old_chars: 0,
            max_new_chars: 0,
            line_number_digits: 2,
            new_content: Arc::from(""),
            semantic: Arc::new(semantic::FileSemanticAnalysis::default()),
            observed_at_millis: 0,
        }
    }

    fn row(change: crate::command::diff::types::ChangeType) -> NativeRow {
        NativeRow {
            old_number: None,
            old_text: String::new(),
            new_number: None,
            new_text: String::new(),
            change,
            old_segments: None,
            new_segments: None,
        }
    }

    #[test]
    fn glob_filters_match_nested_test_files_without_hiding_sources() {
        let matchers = vec![globset::Glob::new("*.test.ts").unwrap().compile_matcher()];

        assert!(is_path_filtered("src/editor/widget.test.ts", &matchers));
        assert!(!is_path_filtered("src/editor/widget.ts", &matchers));
    }

    #[test]
    fn repository_inventory_includes_visible_and_deleted_files() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Repository::init(temp.path()).unwrap();
        std::fs::write(temp.path().join("tracked.rs"), "fn tracked() {}\n").unwrap();
        std::fs::write(temp.path().join("untracked.rs"), "fn untracked() {}\n").unwrap();
        std::fs::write(temp.path().join(".gitignore"), "ignored.rs\n").unwrap();
        std::fs::write(temp.path().join("ignored.rs"), "fn ignored() {}\n").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("tracked.rs")).unwrap();
        index.write().unwrap();

        let paths = load_repository_file_paths(temp.path(), &[file("deleted.rs")]);

        assert!(paths.contains(&"tracked.rs".to_string()));
        assert!(paths.contains(&"untracked.rs".to_string()));
        assert!(paths.contains(&"deleted.rs".to_string()));
        assert!(!paths.contains(&"ignored.rs".to_string()));
    }

    #[test]
    fn workspace_watcher_ignores_git_chatter_and_ignored_output() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Repository::init(temp.path()).unwrap();
        std::fs::write(temp.path().join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir(temp.path().join("target")).unwrap();
        std::fs::write(temp.path().join("target/generated.js"), "generated").unwrap();
        let git_dir = repository.path().to_path_buf();

        assert!(!workspace_event_can_change_review(
            &git_dir.join("objects/pack/pack-123"),
            temp.path(),
            &git_dir,
            &repository,
        ));
        assert!(!workspace_event_can_change_review(
            &temp.path().join("target/generated.js"),
            temp.path(),
            &git_dir,
            &repository,
        ));
        assert!(workspace_event_can_change_review(
            &temp.path().join("src/review.ts"),
            temp.path(),
            &git_dir,
            &repository,
        ));
        assert!(workspace_event_can_change_review(
            &git_dir.join("index"),
            temp.path(),
            &git_dir,
            &repository,
        ));
    }

    #[test]
    fn repository_branch_inventory_groups_local_and_remote_branches() {
        let temp = tempfile::tempdir().unwrap();
        let mut init_options = git2::RepositoryInitOptions::new();
        init_options.initial_head("main");
        let repository = Repository::init_opts(temp.path(), &init_options).unwrap();
        std::fs::write(temp.path().join("tracked.rs"), "fn tracked() {}\n").unwrap();
        let mut index = repository.index().unwrap();
        index.add_path(Path::new("tracked.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Test User", "test@example.com").unwrap();
        let commit_id = repository
            .commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .unwrap();
        let commit = repository.find_commit(commit_id).unwrap();
        repository.branch("feature/local", &commit, false).unwrap();
        repository
            .reference(
                "refs/remotes/origin/feature/remote",
                commit_id,
                false,
                "test remote branch",
            )
            .unwrap();
        repository
            .reference(
                "refs/remotes/origin/HEAD",
                commit_id,
                false,
                "test remote head",
            )
            .unwrap();

        let (local, remote) = repository_branches(temp.path());

        assert_eq!(local, vec!["feature/local", "main"]);
        assert_eq!(remote, vec!["origin/feature/remote"]);
    }

    #[test]
    fn change_type_filters_match_files_using_or_semantics() {
        use crate::command::diff::types::{FileDiff, FileStatus};
        use view::ReviewChangeFamily::{Behaviour, Contract};

        let implementation = model::NativeFile::from_diff(&FileDiff {
            filename: "src/implementation.ts".into(),
            old_content: "function value() { return 1; }".into(),
            new_content: "function value() { return 2; }".into(),
            status: FileStatus::Modified,
            is_binary: false,
        });
        let structural = model::NativeFile::from_diff(&FileDiff {
            filename: "src/contract.ts".into(),
            old_content: "function value(input: string) { return input; }".into(),
            new_content: "function value(input: number) { return input; }".into(),
            status: FileStatus::Modified,
            is_binary: false,
        });
        let unrelated = file("README.md");

        assert!(file_matches_change_types(&unrelated, &HashSet::new()));
        assert!(file_matches_change_types(
            &implementation,
            &HashSet::from([Behaviour])
        ));
        assert!(!file_matches_change_types(
            &structural,
            &HashSet::from([Behaviour])
        ));
        assert!(file_matches_change_types(
            &structural,
            &HashSet::from([Behaviour, Contract])
        ));
        assert!(!file_matches_change_types(
            &unrelated,
            &HashSet::from([Behaviour, Contract])
        ));
    }

    #[test]
    fn change_type_families_classify_non_source_files_from_their_paths() {
        use crate::command::diff::types::FileStatus;
        use view::ReviewChangeFamily::{Configuration, Data, Dependency, Documentation, Test};

        let cases = [
            ("docs/getting-started.md", Documentation),
            ("config/production.yaml", Configuration),
            ("migrations/20260909_add_users.sql", Data),
            ("src/auth/login.test.ts", Test),
            ("package.json", Dependency),
        ];

        for (path, expected) in cases {
            let file = model::NativeFile::summary(path.into(), FileStatus::Modified);
            assert!(
                view::ReviewChangeFamily::for_file(&file).contains(&expected),
                "{path} should have its expected change family",
            );
        }
    }

    #[test]
    fn change_navigation_groups_adjacent_changed_rows_into_one_change() {
        use crate::command::diff::types::ChangeType::{Equal, Insert, Modified};

        let rows = vec![
            row(Equal),
            row(Modified),
            row(Insert),
            row(Equal),
            row(Insert),
            row(Equal),
        ];

        assert_eq!(change_start_rows(&rows), vec![1, 4]);
    }

    #[test]
    fn collapsed_unchanged_sections_render_as_a_single_expandable_row() {
        use crate::command::diff::types::ChangeType::{Equal, Insert};

        let file = model::NativeFile {
            path: "src/example.rs".to_string(),
            status: crate::command::diff::types::FileStatus::Modified,
            additions: 1,
            deletions: 0,
            review_fingerprint: "test-fingerprint".to_string(),
            rows: Arc::from(vec![row(Equal), row(Equal), row(Insert), row(Equal)]),
            max_old_chars: 0,
            max_new_chars: 0,
            line_number_digits: 2,
            new_content: Arc::from(""),
            semantic: Arc::new(semantic::FileSemanticAnalysis::default()),
            observed_at_millis: 0,
        };
        let sections = unchanged_sections(&file.path, &file.rows);
        assert_eq!(sections.len(), 2);
        assert_eq!((sections[0].start_row, sections[0].end_row), (0, 2));
        assert_eq!((sections[1].start_row, sections[1].end_row), (3, 4));

        let collapsed = HashSet::from([sections[0].clone()]);
        let display_rows = display_rows_for_file(&file, &collapsed, &[]);
        assert!(matches!(
            display_rows.as_slice(),
            [
                DiffDisplayRow::CollapsedUnchanged { section },
                DiffDisplayRow::Code { source_row: 2 },
                DiffDisplayRow::Code { source_row: 3 },
            ] if (section.start_row, section.end_row) == (0, 2)
        ));
    }

    #[test]
    fn hiding_unchanged_sections_keeps_each_visited_file_collapsed() {
        use crate::command::diff::types::ChangeType::{Equal, Insert};

        let first = model::NativeFile {
            path: "src/first.rs".to_string(),
            status: crate::command::diff::types::FileStatus::Modified,
            additions: 1,
            deletions: 0,
            review_fingerprint: "first".to_string(),
            rows: Arc::from(vec![row(Equal), row(Insert), row(Equal)]),
            max_old_chars: 0,
            max_new_chars: 0,
            line_number_digits: 2,
            new_content: Arc::from(""),
            semantic: Arc::new(semantic::FileSemanticAnalysis::default()),
            observed_at_millis: 0,
        };
        let second = model::NativeFile {
            path: "src/second.rs".to_string(),
            status: crate::command::diff::types::FileStatus::Modified,
            additions: 1,
            deletions: 0,
            review_fingerprint: "second".to_string(),
            rows: Arc::from(vec![row(Equal), row(Insert)]),
            max_old_chars: 0,
            max_new_chars: 0,
            line_number_digits: 2,
            new_content: Arc::from(""),
            semantic: Arc::new(semantic::FileSemanticAnalysis::default()),
            observed_at_millis: 0,
        };
        let mut collapsed = HashSet::new();

        hide_unchanged_sections_for_file(&first, &mut collapsed, &[]);
        hide_unchanged_sections_for_file(&second, &mut collapsed, &[]);

        assert!(unchanged_sections(&first.path, &first.rows)
            .iter()
            .all(|section| collapsed.contains(section)));
        assert!(unchanged_sections(&second.path, &second.rows)
            .iter()
            .all(|section| collapsed.contains(section)));
    }

    #[test]
    fn tree_view_nests_files_and_hides_collapsed_descendants() {
        let paths = vec![
            "src/desktop/mod.rs".to_string(),
            "src/desktop/model.rs".to_string(),
            "README.md".to_string(),
        ];
        let entries = build_file_tree_entries(&paths, &HashSet::new(), false);
        assert!(matches!(
            entries.as_slice(),
            [
                FileTreeEntry::Directory { name, depth: 0, .. },
                FileTreeEntry::Directory { name: desktop, depth: 1, .. },
                FileTreeEntry::File { index: 0, depth: 2 },
                FileTreeEntry::File { index: 1, depth: 2 },
                FileTreeEntry::File { index: 2, depth: 0 },
            ] if name == "src" && desktop == "desktop"
        ));

        let collapsed = HashSet::from(["src".to_string()]);
        let entries = build_file_tree_entries(&paths, &collapsed, false);
        assert!(matches!(
            entries.as_slice(),
            [
                FileTreeEntry::Directory { name, collapsed: true, .. },
                FileTreeEntry::File { index: 2, depth: 0 },
            ] if name == "src"
        ));
    }

    #[test]
    fn compact_tree_view_combines_unbranched_directory_chains() {
        let paths = vec![
            "packages/business/src/connectors/netsuite/client.rs".to_string(),
            "packages/business/src/connectors/netsuite/types.rs".to_string(),
            "packages/business/src/services/people/profile.rs".to_string(),
            "README.md".to_string(),
        ];

        let entries = build_file_tree_entries(&paths, &HashSet::new(), true);
        assert!(matches!(
            entries.as_slice(),
            [
                FileTreeEntry::Directory { name: root, path: root_path, depth: 0, .. },
                FileTreeEntry::Directory { name: connectors, depth: 1, .. },
                FileTreeEntry::File { index: 0, depth: 2 },
                FileTreeEntry::File { index: 1, depth: 2 },
                FileTreeEntry::Directory { name: services, depth: 1, .. },
                FileTreeEntry::File { index: 2, depth: 2 },
                FileTreeEntry::File { index: 3, depth: 0 },
            ] if root == "packages/business/src"
                && root_path == "packages/business/src"
                && connectors == "connectors/netsuite"
                && services == "services/people"
        ));
    }

    #[test]
    fn sorts_files_alphabetically() {
        let mut files = vec![
            file("src/zebra.rs"),
            file("README.md"),
            file("src/apple.rs"),
        ];
        sort_files_alphabetically(&mut files);
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["README.md", "src/apple.rs", "src/zebra.rs"]
        );
    }

    #[test]
    fn refresh_closes_tabs_for_files_that_are_no_longer_different() {
        let previous_files = vec![file("src/changed.rs"), file("src/remaining.rs")];
        let previous_tabs = vec![
            ReviewTab {
                file_index: 0,
                is_preview: false,
            },
            ReviewTab {
                file_index: 1,
                is_preview: true,
            },
        ];
        let refreshed_files = vec![file("src/remaining.rs")];

        let (tabs, selected) =
            remap_tabs_after_refresh(&previous_files, &previous_tabs, 0, &refreshed_files);

        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].file_index, 0);
        assert!(tabs[0].is_preview);
        assert_eq!(selected, 0);
    }
}
