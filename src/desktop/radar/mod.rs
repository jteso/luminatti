//! Snapshot-based file dependency context, with changed callable annotations.
mod calls;
pub(super) mod bubbles;
mod layout;
mod parse;
mod resolve;
mod tala;
#[cfg(test)]
mod tests;

use crate::commit_reference::CommitReference;
use git2::{ObjectType, Repository, Tree};
use globset::Glob;
pub(super) use layout::Diagram;
pub(super) const FUNCTION_ROW_HEIGHT: f32 = layout::ROW;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    Added,
    Removed,
    Modified,
    Unchanged,
}
impl Change {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Removed => "D",
            Self::Modified => "M",
            Self::Unchanged => "",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct FunctionNode {
    pub name: String,
    pub line: usize,
    pub change: Change,
}

#[derive(Clone, Debug)]
pub(super) struct FileNode {
    pub path: String,
    pub package: String,
    pub change: Change,
    pub functions: Vec<FunctionNode>,
    pub entry: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Edge {
    pub from: String,
    pub to: String,
    pub change: Change,
}

type FunctionId = (String, String);
#[derive(Clone, Debug)]
pub(super) struct CallEdge {
    pub from: FunctionId,
    pub to: FunctionId,
    pub change: Change,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Graph {
    pub files: BTreeMap<String, FileNode>,
    pub edges: Vec<Edge>,
    pub calls: Vec<CallEdge>,
    pub skipped_files: usize,
    pub unresolved: usize,
}

struct LayoutRequest {
    generation: u64,
    graph: Arc<Graph>,
    expand_all: bool,
    expanded: BTreeSet<String>,
    cycles: BTreeSet<String>,
    files: BTreeSet<String>,
}

pub(super) struct RadarState {
    pub use_bubbles: bool,
    pub open: bool,
    pub active: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub diagram: Option<Arc<Diagram>>,
    graph: Option<Arc<Graph>>,
    pub expanded: BTreeSet<String>,
    pub cycles: BTreeSet<String>,
    pub expanded_files: BTreeSet<String>,
    pub expand_all: bool,
    pub focused_files: BTreeSet<String>,
    generation: u64,
    layout_generation: u64,
    pub arranging: bool,
    layout_requests: mpsc::Sender<LayoutRequest>,
    layout_events: mpsc::Receiver<(u64, Diagram)>,
    requests: mpsc::Sender<(u64, PathBuf, Option<CommitReference>, Vec<String>)>,
    events: mpsc::Receiver<(u64, Result<Graph, String>)>,
}

impl Default for RadarState {
    fn default() -> Self {
        let (sender, events) = mpsc::channel();
        let (requests, receiver) =
            mpsc::channel::<(u64, PathBuf, Option<CommitReference>, Vec<String>)>();
        std::thread::spawn(move || {
            while let Ok(mut request) = receiver.recv() {
                while let Ok(latest) = receiver.try_recv() {
                    request = latest;
                }
                let (generation, root, reference, filters) = request;
                let result = load(&root, reference.as_ref()).map(|mut graph| {
                    filter_graph(&mut graph, &filters);
                    graph
                });
                if sender.send((generation, result)).is_err() {
                    break;
                }
            }
        });
        let (layout_requests, receiver) = mpsc::channel::<LayoutRequest>();
        let (sender, layout_events) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(mut request) = receiver.recv() {
                while let Ok(latest) = receiver.try_recv() {
                    request = latest;
                }
                let diagram = Diagram::with_tala(
                    &request.graph,
                    request.expand_all,
                    &request.expanded,
                    &request.cycles,
                    &request.files,
                );
                if sender.send((request.generation, diagram)).is_err() {
                    break;
                }
            }
        });
        Self {
            use_bubbles: false,
            open: false,
            active: false,
            loading: false,
            error: None,
            diagram: None,
            graph: None,
            expanded: BTreeSet::new(),
            cycles: BTreeSet::new(),
            expanded_files: BTreeSet::new(),
            expand_all: false,
            focused_files: BTreeSet::new(),
            generation: 0,
            layout_generation: 0,
            arranging: false,
            layout_requests,
            layout_events,
            requests,
            events,
        }
    }
}

impl RadarState {
    pub fn has_graph(&self) -> bool {
        self.graph.is_some()
    }
    pub fn dependent_counts(&self) -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();
        if let Some(graph) = &self.graph {
            for edge in &graph.edges {
                *counts.entry(edge.to.clone()).or_insert(0) += 1;
            }
        }
        counts
    }
    pub fn refresh(
        &mut self,
        root: PathBuf,
        reference: Option<CommitReference>,
        filters: Vec<String>,
    ) {
        self.generation = self.generation.wrapping_add(1);
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.arranging = false;
        self.error = None;
        self.loading = self.open;
        if !self.open {
            self.diagram = None;
            self.graph = None;
            return;
        }
        if self
            .requests
            .send((self.generation, root, reference, filters))
            .is_err()
        {
            self.loading = false;
            self.error = Some("Dependency worker stopped".into());
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((generation, result)) = self.events.try_recv() {
            if generation != self.generation {
                continue;
            }
            self.loading = false;
            match result {
                Ok(graph) => {
                    self.focused_files
                        .retain(|path| graph.files.contains_key(path));
                    self.graph = Some(Arc::new(graph));
                    if !self.use_bubbles {
                        self.relayout();
                    }
                    self.error = None;
                }
                Err(error) => {
                    self.layout_generation = self.layout_generation.wrapping_add(1);
                    self.arranging = false;
                    self.error = Some(error);
                    self.diagram = None;
                    self.graph = None;
                }
            }
            changed = true;
        }
        while let Ok((generation, diagram)) = self.layout_events.try_recv() {
            if generation != self.layout_generation {
                continue;
            }
            self.arranging = false;
            if diagram.tala {
                self.diagram = Some(Arc::new(diagram));
            }
            changed = true;
        }
        changed
    }
    fn relayout(&mut self) {
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.arranging = false;
        if let Some(graph) = &self.graph {
            let diagram = Diagram::with_files(
                graph,
                self.expand_all,
                &self.expanded,
                &self.cycles,
                &self.expanded_files,
            );
            self.focused_files
                .retain(|path| diagram.groups.iter().any(|group| &group.path == path));
            self.diagram = Some(Arc::new(diagram));
            if tala::executable().is_some() {
                self.arranging = self
                    .layout_requests
                    .send(LayoutRequest {
                        generation: self.layout_generation,
                        graph: graph.clone(),
                        expand_all: self.expand_all,
                        expanded: self.expanded.clone(),
                        cycles: self.cycles.clone(),
                        files: self.expanded_files.clone(),
                    })
                    .is_ok();
            }
        }
    }
    pub fn toggle_all(&mut self) {
        self.expand_all = !self.diagram.as_ref().is_some_and(|d| d.all_expanded);
        self.expanded.clear();
        self.relayout();
    }
    pub fn toggle_chain(&mut self, key: String) {
        // With expand-all on, the set contains exceptions (collapsed chains).
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        self.relayout();
    }
    pub fn toggle_cycle(&mut self, key: String) {
        if !self.cycles.remove(&key) {
            self.cycles.insert(key);
        }
        self.relayout();
    }
    pub fn toggle_file(&mut self, path: String) {
        if !self.expanded_files.remove(&path) {
            self.expanded_files.insert(path);
        }
        self.relayout();
    }
    pub fn toggle_dependency_focus(&mut self, path: String) {
        if !self.focused_files.remove(&path) {
            self.focused_files.insert(path);
        }
    }
    pub fn clear_dependency_focus(&mut self) {
        self.focused_files.clear();
    }
    pub fn dependency_focus(&self) -> Option<DependencyFocus> {
        if self.focused_files.is_empty() {
            return None;
        }
        let graph = self.graph.as_deref()?;
        let mut combined = DependencyFocus::default();
        for path in &self.focused_files {
            if let Some(focus) = dependency_focus(graph, path) {
                combined.files.extend(focus.files);
                combined.edges.extend(focus.edges);
            }
        }
        Some(combined)
    }

    pub fn focus_for(&self, path: &str) -> Option<DependencyFocus> {
        dependency_focus(self.graph.as_deref()?, path)
    }

    pub fn review_focus_for(
        &self,
        path: &str,
        changed: &BTreeSet<String>,
        changed_only: bool,
    ) -> Option<DependencyFocus> {
        review_dependency_focus(self.graph.as_deref()?, path, changed, changed_only)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DependencyFocus {
    pub files: BTreeSet<String>,
    pub edges: BTreeSet<(String, String)>,
}

/// Follow imports downward and importer paths upward in each snapshot
/// independently. This keeps a removed import from being joined to an added
/// import and presented as one historical path.
fn dependency_focus(graph: &Graph, path: &str) -> Option<DependencyFocus> {
    graph.files.contains_key(path).then_some(())?;
    let mut focus = DependencyFocus::default();
    focus.files.insert(path.into());
    for before in [true, false] {
        let file_change = graph.files[path].change;
        if (before && file_change == Change::Added) || (!before && file_change == Change::Removed) {
            continue;
        }
        let mut queue = VecDeque::from([path]);
        let mut visited = BTreeSet::new();
        while let Some(from) = queue.pop_front() {
            if !visited.insert(from) {
                continue;
            }
            for edge in graph.edges.iter().filter(|edge| {
                edge.from == from
                    && !((before && edge.change == Change::Added)
                        || (!before && edge.change == Change::Removed))
            }) {
                focus.files.insert(edge.to.clone());
                focus.edges.insert((edge.from.clone(), edge.to.clone()));
                queue.push_back(edge.to.as_str());
            }
        }

        let mut queue = VecDeque::from([path]);
        let mut visited = BTreeSet::new();
        while let Some(to) = queue.pop_front() {
            if !visited.insert(to) {
                continue;
            }
            for edge in graph.edges.iter().filter(|edge| {
                edge.to == to
                    && !((before && edge.change == Change::Added)
                        || (!before && edge.change == Change::Removed))
            }) {
                focus.files.insert(edge.from.clone());
                focus.edges.insert((edge.from.clone(), edge.to.clone()));
                queue.push_back(edge.from.as_str());
            }
        }
    }
    Some(focus)
}

/// Keep one shortest, real import route between the selected file and each
/// changed file. Search old and new snapshots separately so a route cannot
/// accidentally join edges that never existed together.
fn review_dependency_focus(
    graph: &Graph,
    path: &str,
    changed: &BTreeSet<String>,
    changed_only: bool,
) -> Option<DependencyFocus> {
    let selected = graph.files.get(path)?;
    let mut routes: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for before in [false, true] {
        if (before && selected.change == Change::Added)
            || (!before && selected.change == Change::Removed)
        {
            continue;
        }
        for outgoing in [true, false] {
            let mut neighbors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for edge in &graph.edges {
                if (before && edge.change == Change::Added)
                    || (!before && edge.change == Change::Removed)
                    || (changed_only
                        && (!changed.contains(&edge.from) || !changed.contains(&edge.to)))
                {
                    continue;
                }
                let (from, to) = if outgoing {
                    (&edge.from, &edge.to)
                } else {
                    (&edge.to, &edge.from)
                };
                neighbors.entry(from.clone()).or_default().insert(to.clone());
            }
            let mut parents = BTreeMap::from([(path.to_string(), path.to_string())]);
            let mut queue = VecDeque::from([path.to_string()]);
            while let Some(current) = queue.pop_front() {
                if let Some(next) = neighbors.get(&current) {
                    for neighbor in next {
                        if !parents.contains_key(neighbor) {
                            parents.insert(neighbor.clone(), current.clone());
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
            for target in changed.iter().filter(|target| target.as_str() != path) {
                if !parents.contains_key(target) {
                    continue;
                }
                let mut current = target.clone();
                let mut route = Vec::new();
                while current != path {
                    let previous = parents[&current].clone();
                    route.push(if outgoing {
                        (previous.clone(), current)
                    } else {
                        (current, previous.clone())
                    });
                    current = previous;
                }
                if routes.get(target).is_none_or(|existing| route.len() < existing.len()) {
                    routes.insert(target.clone(), route);
                }
            }
        }
    }
    let mut focus = DependencyFocus::default();
    focus.files.insert(path.to_string());
    for route in routes.into_values() {
        for (from, to) in route {
            focus.files.insert(from.clone());
            focus.files.insert(to.clone());
            focus.edges.insert((from, to));
        }
    }
    Some(focus)
}

fn filter_graph(graph: &mut Graph, patterns: &[String]) {
    let matchers = patterns
        .iter()
        .filter_map(|p| Glob::new(p).ok())
        .map(|g| g.compile_matcher())
        .collect::<Vec<_>>();
    let visible = |path: &str| !matchers.iter().any(|m| m.is_match(path));
    graph.files.retain(|path, _| visible(path));
    graph.edges.retain(|e| visible(&e.from) && visible(&e.to));
    graph
        .calls
        .retain(|e| visible(&e.from.0) && visible(&e.to.0));
    calls::retain_context(graph);
}

#[derive(Default)]
struct Snapshot {
    sources: BTreeMap<String, String>,
    hashes: BTreeMap<String, String>,
    modules: BTreeMap<String, parse::Module>,
    metadata: BTreeMap<String, serde_json::Value>,
    skipped: BTreeSet<String>,
}
fn supported(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    (matches!(
        Path::new(path).extension().and_then(|s| s.to_str()),
        Some("ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs")
    ) || name == "package.json"
        || name == "package-lock.json"
        || name == "luminatti.review.json"
        || name == "luminatti.review-validation.json"
        || (name.starts_with("tsconfig") && name.ends_with(".json"))
        || name == "jsconfig.json")
        && !path
            .split('/')
            .any(|part| matches!(part, "node_modules" | ".git"))
}
fn insert(snapshot: &mut Snapshot, path: &str, bytes: &[u8]) {
    use sha2::{Digest, Sha256};
    snapshot.hashes.insert(path.into(), format!("{:x}", Sha256::digest(bytes)));
    let Ok(source) = std::str::from_utf8(bytes) else {
        snapshot.skipped.insert(path.into());
        return;
    };
    snapshot.sources.insert(path.into(), source.into());
    if path.ends_with(".json") || path.ends_with(".jsonc") {
        if let Some(value) = resolve::json_config(source) {
            snapshot.metadata.insert(path.into(), value);
        } else {
            snapshot.skipped.insert(path.into());
        }
        return;
    }
    if !matches!(Path::new(path).extension().and_then(|s| s.to_str()), Some("ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs")) {
        return;
    }
    let module = parse::parse(path, source);
    if module.parse_error {
        snapshot.skipped.insert(path.into());
    }
    snapshot.modules.insert(path.into(), module);
}
fn capture_path(path: &str, extra: Option<&BTreeSet<String>>) -> bool {
    supported(path) || extra.is_some_and(|paths| paths.contains(path) || path.ends_with(".json") || path.ends_with(".jsonc"))
}
fn tree_snapshot(repo: &Repository, tree: &Tree<'_>, extra: Option<&BTreeSet<String>>) -> Result<Snapshot, String> {
    fn walk(
        repo: &Repository,
        tree: &Tree<'_>,
        prefix: &str,
        snapshot: &mut Snapshot,
        extra: Option<&BTreeSet<String>>,
    ) -> Result<(), String> {
        for entry in tree {
            let Some(name) = entry.name() else { continue };
            let path = format!("{prefix}{name}");
            if entry.kind() == Some(ObjectType::Tree) {
                if matches!(name, "node_modules" | ".git") {
                    continue;
                }
                let tree = repo
                    .find_tree(entry.id())
                    .map_err(|error| error.to_string())?;
                walk(repo, &tree, &format!("{path}/"), snapshot, extra)?;
            } else if capture_path(&path, extra) && matches!(entry.filemode(), 0o100644 | 0o100755) {
                let blob = repo
                    .find_blob(entry.id())
                    .map_err(|error| error.to_string())?;
                insert(snapshot, &path, blob.content());
            }
        }
        Ok(())
    }
    let mut snapshot = Snapshot::default();
    walk(repo, tree, "", &mut snapshot, extra)?;
    Ok(snapshot)
}

fn working_snapshot(root: &Path, extra: Option<&BTreeSet<String>>) -> Result<Snapshot, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    let mut snapshot = Snapshot::default();
    for path in output.stdout.split(|byte| *byte == 0) {
        let Ok(path) = std::str::from_utf8(path) else {
            continue;
        };
        if !capture_path(path, extra) || snapshot.sources.contains_key(path) {
            continue;
        }
        let absolute = root.join(path);
        // Never follow repository symlinks into unrelated directories.
        match std::fs::symlink_metadata(&absolute) {
            Ok(meta) if meta.is_file() => match std::fs::read(&absolute) {
                Ok(source) => insert(&mut snapshot, path, &source),
                Err(_) => {
                    snapshot.skipped.insert(path.into());
                }
            },
            Ok(_) => {
                snapshot.skipped.insert(path.into());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                snapshot.skipped.insert(path.into());
            }
        }
    }
    Ok(snapshot)
}

fn revision_tree<'a>(repo: &'a Repository, reference: &str) -> Result<Tree<'a>, String> {
    repo.revparse_single(reference)
        .and_then(|object| object.peel_to_tree())
        .map_err(|error| error.to_string())
}

fn load_snapshots(root: &Path, reference: Option<&CommitReference>, extra: Option<&BTreeSet<String>>) -> Result<(Snapshot, Snapshot), String> {
    let repo = Repository::discover(root).map_err(|error| error.to_string())?;
    let tree_snapshot = |repo: &Repository, tree: &Tree<'_>| tree_snapshot(repo, tree, extra);
    let working_snapshot = |root: &Path| working_snapshot(root, extra);
    let (before, after) = match reference {
        None => {
            let before = match repo.head() {
                Ok(head) => tree_snapshot(
                    &repo,
                    &head.peel_to_tree().map_err(|error| error.to_string())?,
                )?,
                Err(error)
                    if matches!(
                        error.code(),
                        git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                    ) =>
                {
                    Snapshot::default()
                }
                Err(error) => return Err(error.to_string()),
            };
            (before, working_snapshot(root)?)
        }
        Some(CommitReference::Single(reference)) => {
            let commit = repo
                .revparse_single(reference)
                .and_then(|object| object.peel_to_commit())
                .map_err(|error| error.to_string())?;
            let before = if commit.parent_count() == 0 {
                Snapshot::default()
            } else {
                tree_snapshot(
                    &repo,
                    &commit
                        .parent(0)
                        .and_then(|commit| commit.tree())
                        .map_err(|error| error.to_string())?,
                )?
            };
            (
                before,
                tree_snapshot(&repo, &commit.tree().map_err(|error| error.to_string())?)?,
            )
        }
        Some(CommitReference::Range { from, to }) => (
            tree_snapshot(&repo, &revision_tree(&repo, from)?)?,
            tree_snapshot(&repo, &revision_tree(&repo, to)?)?,
        ),
        Some(CommitReference::TripleDots { from, to }) => {
            let from = repo
                .revparse_single(from)
                .and_then(|object| object.peel_to_commit())
                .map_err(|error| error.to_string())?;
            let to = repo
                .revparse_single(to)
                .and_then(|object| object.peel_to_commit())
                .map_err(|error| error.to_string())?;
            let base = repo
                .merge_base(from.id(), to.id())
                .map_err(|error| error.to_string())?;
            (
                tree_snapshot(
                    &repo,
                    &repo
                        .find_commit(base)
                        .and_then(|commit| commit.tree())
                        .map_err(|error| error.to_string())?,
                )?,
                tree_snapshot(&repo, &to.tree().map_err(|error| error.to_string())?)?,
            )
        }
        Some(CommitReference::RangeToWorkingTree { from }) => (
            tree_snapshot(&repo, &revision_tree(&repo, from)?)?,
            working_snapshot(root)?,
        ),
    };
    Ok((before, after))
}

fn load(root: &Path, reference: Option<&CommitReference>) -> Result<Graph, String> {
    let (before, after) = load_snapshots(root, reference, None)?;
    Ok(compare(before, after))
}

/// Share revision semantics without Radar's presentation filters or layout.
pub(super) fn review_sources(root: &Path, reference: Option<&CommitReference>, paths: &[String]) -> Result<(BTreeMap<String, String>, BTreeMap<String, String>, Vec<String>, BTreeMap<String, String>, BTreeMap<String, String>), String> {
    let extra = paths.iter().cloned().collect();
    let (before, after) = load_snapshots(root, reference, Some(&extra))?;
    if reference.is_none() || matches!(reference, Some(CommitReference::RangeToWorkingTree { .. })) {
        let check = working_snapshot(root, Some(&extra))?;
        if check.hashes != after.hashes || check.skipped != after.skipped {
            return Err("Files changed during capture; waiting for the next refresh".into());
        }
    }
    let skipped = before.skipped.union(&after.skipped).cloned().collect();
    Ok((before.sources, after.sources, skipped, before.hashes, after.hashes))
}

fn compare(mut before: Snapshot, mut after: Snapshot) -> Graph {
    let skipped = before
        .skipped
        .union(&after.skipped)
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in &skipped {
        before.modules.remove(path);
        after.modules.remove(path);
        before.metadata.remove(path);
        after.metadata.remove(path);
    }
    let old_resolver = resolve::Resolver::new(&before);
    let new_resolver = resolve::Resolver::new(&after);
    let (old_edges, old_unknown) = old_resolver.edges();
    let (new_edges, new_unknown) = new_resolver.edges();
    let old_calls = calls::edges(&before, &old_resolver);
    let new_calls = calls::edges(&after, &new_resolver);
    let mut graph = Graph {
        skipped_files: skipped.len(),
        unresolved: old_unknown.union(&new_unknown).count(),
        ..Graph::default()
    };
    for path in before
        .modules
        .keys()
        .chain(after.modules.keys())
        .collect::<BTreeSet<_>>()
    {
        let old = before.modules.get(path);
        let new = after.modules.get(path);
        let change = match (old, new) {
            (None, Some(_)) => Change::Added,
            (Some(_), None) => Change::Removed,
            (Some(a), Some(b)) if a.fingerprint != b.fingerprint => Change::Modified,
            _ => Change::Unchanged,
        };
        let mut functions = Vec::new();
        for name in old
            .into_iter()
            .flat_map(|m| m.functions.keys())
            .chain(new.into_iter().flat_map(|m| m.functions.keys()))
            .collect::<BTreeSet<_>>()
        {
            let a = old.and_then(|m| m.functions.get(name));
            let b = new.and_then(|m| m.functions.get(name));
            let change = match (a, b) {
                (None, Some(_)) => Change::Added,
                (Some(_), None) => Change::Removed,
                (Some(a), Some(b)) if a.signature != b.signature || a.body != b.body => {
                    Change::Modified
                }
                _ => Change::Unchanged,
            };
            functions.push(FunctionNode {
                name: name.clone(),
                line: b.or(a).unwrap().line,
                change,
            });
        }
        functions.sort_by_key(|f| f.line);
        graph.files.insert(
            path.clone(),
            FileNode {
                path: path.clone(),
                package: if new.is_some() {
                    new_resolver.package(path)
                } else {
                    old_resolver.package(path)
                },
                change,
                functions,
                entry: old_resolver.is_entry(path) || new_resolver.is_entry(path),
            },
        );
    }
    for (from, to) in old_edges.union(&new_edges) {
        let pair = (from.clone(), to.clone());
        graph.edges.push(Edge {
            from: from.clone(),
            to: to.clone(),
            change: if !old_edges.contains(&pair) {
                Change::Added
            } else if !new_edges.contains(&pair) {
                Change::Removed
            } else {
                Change::Unchanged
            },
        });
    }
    for (from, to) in old_calls.union(&new_calls) {
        let pair = (from.clone(), to.clone());
        graph.calls.push(CallEdge {
            from: from.clone(),
            to: to.clone(),
            change: if !old_calls.contains(&pair) {
                Change::Added
            } else if !new_calls.contains(&pair) {
                Change::Removed
            } else {
                Change::Unchanged
            },
        });
    }
    calls::retain_context(&mut graph);
    graph
}
