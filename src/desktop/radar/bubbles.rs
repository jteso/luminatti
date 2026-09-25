//! Bounded, deterministic circle packing for the repository overview.
//! The worker owns filesystem sampling and layout; paint only reads the result.

use super::{Change, DependencyFocus};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

#[derive(Clone)]
pub(in super::super) struct FileInput {
    pub path: String,
    pub change: Change,
    pub churn: usize,
}

#[derive(Clone)]
pub(in super::super) struct Bubble {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub label: String,
    pub path: String,
    pub change: Change,
    pub contains_changes: bool,
    pub importance: f32,
    pub dependents: usize,
    pub lines: usize,
    pub directory: bool,
    pub subtree_end: usize,
}

pub(in super::super) fn focus_radius(node: &Bubble, scale: f32, selected: bool) -> f32 {
    let minimum = if selected {
        12.
    } else if node.change == Change::Unchanged {
        7.5
    } else {
        9.5
    };
    (node.radius * scale).max(minimum)
}

#[derive(Clone, Default)]
pub(in super::super) struct Map {
    pub nodes: Vec<Bubble>,
    pub radius: f32,
    pub files: usize,
    pub changed: usize,
}

#[derive(Clone)]
pub(in super::super) struct BubbleFocus {
    pub files: BTreeSet<String>,
    pub edges: Vec<(String, String, usize)>,
    pub stages: usize,
    positions: HashMap<String, (f32, f32)>,
    origins: HashMap<String, (f32, f32)>,
}

impl BubbleFocus {
    pub fn new(selected: &str, focus: DependencyFocus) -> Self {
        let mut adjacent: HashMap<&str, Vec<&str>> = HashMap::new();
        for (from, to) in &focus.edges {
            adjacent.entry(from).or_default().push(to);
            adjacent.entry(to).or_default().push(from);
        }
        let mut distance = HashMap::from([(selected.to_string(), 0_usize)]);
        let mut queue = VecDeque::from([selected.to_string()]);
        while let Some(path) = queue.pop_front() {
            let depth = distance[&path];
            for &next in adjacent.get(path.as_str()).into_iter().flatten() {
                if !distance.contains_key(next) {
                    distance.insert(next.to_string(), depth + 1);
                    queue.push_back(next.to_string());
                }
            }
        }
        let mut edges = focus
            .edges
            .into_iter()
            .map(|(from, to)| {
                let stage = distance
                    .get(&from)
                    .copied()
                    .unwrap_or(0)
                    .min(distance.get(&to).copied().unwrap_or(0));
                (from, to, stage)
            })
            .collect::<Vec<_>>();
        edges.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
        let stages = edges.last().map_or(0, |edge| edge.2 + 1);
        Self {
            files: focus.files,
            edges,
            stages,
            positions: HashMap::new(),
            origins: HashMap::new(),
        }
    }

    pub fn arrange(
        &mut self,
        map: &Map,
        selected: &str,
        fit: f32,
        previous: Option<&Self>,
        previous_progress: f32,
    ) {
        let mut downstream: HashMap<String, Vec<String>> = HashMap::new();
        let mut upstream: HashMap<String, Vec<String>> = HashMap::new();
        for (from, to, _) in &self.edges {
            downstream.entry(from.clone()).or_default().push(to.clone());
            upstream.entry(to.clone()).or_default().push(from.clone());
        }
        fn depths(start: &str, neighbors: &HashMap<String, Vec<String>>) -> HashMap<String, usize> {
            let mut distance = HashMap::from([(start.to_string(), 0_usize)]);
            let mut queue = VecDeque::from([start.to_string()]);
            while let Some(path) = queue.pop_front() {
                let depth = distance[&path];
                for next in neighbors.get(&path).into_iter().flatten() {
                    if !distance.contains_key(next) {
                        distance.insert(next.clone(), depth + 1);
                        queue.push_back(next.clone());
                    }
                }
            }
            distance
        }
        let down = depths(selected, &downstream);
        let up = depths(selected, &upstream);
        let mut columns: BTreeMap<i32, Vec<&Bubble>> = BTreeMap::new();
        for node in map
            .nodes
            .iter()
            .filter(|node| !node.directory && self.files.contains(&node.path))
        {
            let rank = match (down.get(&node.path), up.get(&node.path)) {
                (Some(&forward), Some(&backward)) if backward < forward => -(backward as i32),
                (Some(&forward), _) => forward as i32,
                (_, Some(&backward)) => -(backward as i32),
                _ => 0,
            };
            columns.entry(rank).or_default().push(node);
            self.origins.insert(
                node.path.clone(),
                previous
                    .map(|focus| focus.position(node, previous_progress))
                    .unwrap_or((node.x, node.y)),
            );
        }
        let column_gap = 225. / fit;
        let row_gap = 76. / fit;
        for (rank, nodes) in &mut columns {
            nodes.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.path.cmp(&b.path)));
            let middle = (nodes.len() as f32 - 1.) * 0.5;
            for (index, node) in nodes.iter().enumerate() {
                self.positions.insert(
                    node.path.clone(),
                    (*rank as f32 * column_gap, (index as f32 - middle) * row_gap),
                );
            }
        }
    }

    pub fn position(&self, node: &Bubble, progress: f32) -> (f32, f32) {
        let Some(&(target_x, target_y)) = self.positions.get(&node.path) else {
            return (node.x, node.y);
        };
        let &(start_x, start_y) = self.origins.get(&node.path).unwrap_or(&(node.x, node.y));
        (
            start_x + (target_x - start_x) * progress,
            start_y + (target_y - start_y) * progress,
        )
    }

    pub fn target(&self, node: &Bubble) -> (f32, f32) {
        self.positions
            .get(&node.path)
            .copied()
            .unwrap_or((node.x, node.y))
    }
}

#[derive(Default)]
struct Folder {
    folders: BTreeMap<String, Folder>,
    files: Vec<FileInput>,
}

struct Request {
    revision: u64,
    root: PathBuf,
    files: Vec<FileInput>,
    dependents: HashMap<String, usize>,
}

pub(in super::super) struct BubbleState {
    pub map: Option<Arc<Map>>,
    changed_map: Option<Arc<Map>>,
    pub loading: bool,
    pub zoom: f32,
    pub pan: (f32, f32),
    pub selected: Option<String>,
    pub history: Vec<(String, bool)>,
    pub focus: Option<BubbleFocus>,
    pub show_all_dependencies: bool,
    pub show_changed_only: bool,
    pub layout_progress: f32,
    pub layout_epoch: u64,
    pub reveal: f32,
    pub hovered: Option<String>,
    pub hover_position: (f32, f32),
    pub view_menu_open: bool,
    pub pulse: f32,
    pub drag_start: Option<(f32, f32)>,
    pub drag_pan: (f32, f32),
    pub drag_moved: bool,
    pub viewport: Arc<Mutex<(f32, f32, f32, f32)>>,
    pub pulse_epoch: u64,
    pub camera_epoch: u64,
    pub reveal_epoch: u64,
    revision: u64,
    sender: mpsc::Sender<Request>,
    receiver: mpsc::Receiver<(u64, Map, Map)>,
}

impl Default for BubbleState {
    fn default() -> Self {
        let (sender, requests) = mpsc::channel::<Request>();
        let (results, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(mut request) = requests.recv() {
                while let Ok(latest) = requests.try_recv() {
                    request = latest;
                }
                let changed_files = request
                    .files
                    .iter()
                    .filter(|file| file.change != Change::Unchanged)
                    .cloned()
                    .collect();
                let map = build(&request.root, request.files, &request.dependents, false);
                let changed_map = build(&request.root, changed_files, &request.dependents, true);
                if results.send((request.revision, map, changed_map)).is_err() {
                    break;
                }
            }
        });
        Self {
            map: None,
            changed_map: None,
            loading: false,
            zoom: 1.,
            pan: (0., 0.),
            selected: None,
            history: Vec::new(),
            focus: None,
            show_all_dependencies: false,
            show_changed_only: false,
            layout_progress: 1.,
            layout_epoch: 0,
            reveal: 0.,
            hovered: None,
            hover_position: (0., 0.),
            view_menu_open: false,
            pulse: 0.,
            drag_start: None,
            drag_pan: (0., 0.),
            drag_moved: false,
            viewport: Arc::new(Mutex::new((0., 0., 1., 1.))),
            pulse_epoch: 0,
            camera_epoch: 0,
            reveal_epoch: 0,
            revision: 0,
            sender,
            receiver,
        }
    }
}

impl BubbleState {
    pub fn current_map(&self) -> Option<&Arc<Map>> {
        if self.show_changed_only {
            self.changed_map.as_ref()
        } else {
            self.map.as_ref()
        }
    }

    pub fn clear_hover_emphasis(&mut self) {
        self.hovered = None;
    }

    pub fn display_map(&self) -> Option<Arc<Map>> {
        self.current_map().cloned()
    }

    pub fn refresh(
        &mut self,
        root: PathBuf,
        files: Vec<FileInput>,
        dependents: HashMap<String, usize>,
    ) {
        self.revision = self.revision.wrapping_add(1);
        self.loading = true;
        let _ = self.sender.send(Request {
            revision: self.revision,
            root,
            files,
            dependents,
        });
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((revision, map, changed_map)) = self.receiver.try_recv() {
            if revision == self.revision {
                self.map = Some(Arc::new(map));
                self.changed_map = Some(Arc::new(changed_map));
                self.loading = false;
                self.clear_hover_emphasis();
                changed = true;
            }
        }
        changed
    }

    pub fn transform(&self) -> (f32, f32, f32) {
        let (_, _, width, height) = *self.viewport.lock().unwrap();
        let radius = self.current_map().map_or(1., |map| map.radius.max(1.));
        let fit = (width / (2. * radius + 48.)).min(height / (2. * radius + 48.));
        (
            fit * self.zoom,
            width * 0.5 + self.pan.0,
            height * 0.5 + self.pan.1,
        )
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<Bubble> {
        let map = self.display_map()?;
        let (scale, cx, cy) = self.transform();
        if let Some(focus) = &self.focus {
            for node in map.nodes.iter().rev().filter(|node| {
                !node.directory
                    && focus.files.contains(&node.path)
                    && (!self.show_changed_only || node.contains_changes)
            }) {
                let (nx, ny) = focus.position(node, self.layout_progress);
                let radius = focus_radius(
                    node,
                    scale,
                    self.selected.as_deref() == Some(node.path.as_str()),
                );
                let dx = x - (cx + nx * scale);
                let dy = y - (cy + ny * scale);
                if dx * dx + dy * dy <= radius * radius {
                    return Some(node.clone());
                }
            }
            if self.show_changed_only {
                return None;
            }
        }
        let (x, y) = ((x - cx) / scale, (y - cy) / scale);
        let mut index = 0;
        let mut found = None;
        while index < map.nodes.len() {
            let node = &map.nodes[index];
            if self.show_changed_only && !node.contains_changes {
                index = if node.directory {
                    node.subtree_end
                } else {
                    index + 1
                };
                continue;
            }
            let dx = x - node.x;
            let dy = y - node.y;
            if dx * dx + dy * dy <= node.radius * node.radius {
                if !self
                    .focus
                    .as_ref()
                    .is_some_and(|focus| focus.files.contains(&node.path))
                {
                    found = Some(node.clone());
                }
                index += 1;
            } else {
                index = if node.directory {
                    node.subtree_end
                } else {
                    index + 1
                };
            }
        }
        if self.focus.is_some() {
            found.filter(|node| !node.directory)
        } else {
            found
        }
    }
}

fn estimate_lines(root: &Path, path: &str) -> usize {
    let absolute = root.join(path);
    let Ok(metadata) = std::fs::symlink_metadata(&absolute) else {
        return 0;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return 0;
    }
    let Ok(mut file) = File::open(absolute) else {
        return 0;
    };
    let mut sample = [0_u8; 4 * 1024];
    let Ok(read) = file.read(&mut sample) else {
        return 0;
    };
    if read == 0 || sample[..read].contains(&0) {
        return 0;
    }
    let breaks = sample[..read].iter().filter(|&&byte| byte == b'\n').count();
    ((breaks.max(1) as f64 * metadata.len() as f64 / read as f64) as usize).min(1_000_000)
}

fn file_radius(lines: usize, dependents: usize, churn: usize, change: Change) -> (f32, f32) {
    // A log score gives dependency reach priority while preventing generated
    // files or very large diffs from swallowing the entire map.
    let score = 0.46 * (1. + dependents as f32).ln()
        + 0.24 * (1. + lines as f32).ln()
        + 0.20 * (1. + churn as f32).ln()
        + if change == Change::Unchanged {
            0.
        } else {
            0.65
        };
    let radius = 10. + 4. * (score / (score + 3.)).clamp(0., 1.);
    (if change == Change::Unchanged { radius } else { radius * 2. }, score)
}

fn build(
    root: &Path,
    files: Vec<FileInput>,
    dependents: &HashMap<String, usize>,
    compact: bool,
) -> Map {
    let mut tree = Folder::default();
    let changed = files
        .iter()
        .filter(|f| f.change != Change::Unchanged)
        .count();
    let count = files.len();
    for file in files {
        let mut folder = &mut tree;
        let mut parts = file.path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                folder = folder.folders.entry(part.to_owned()).or_default();
            } else {
                folder.files.push(file.clone());
            }
        }
    }
    let mut nodes = Vec::with_capacity(count * 2);
    let radius = place_folder(&tree, "", "", root, dependents, &mut nodes, false, compact);
    Map {
        nodes,
        radius,
        files: count,
        changed,
    }
}

fn hex_position(index: usize) -> (f32, f32) {
    if index == 0 {
        return (0., 0.);
    }
    let mut remaining = index - 1;
    let mut ring = 1;
    while remaining >= 6 * ring {
        remaining -= 6 * ring;
        ring += 1;
    }
    let side = remaining / ring;
    let step = remaining % ring;
    let vertices = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];
    let (aq, ar) = vertices[side];
    let (bq, br) = vertices[(side + 1) % 6];
    let q = aq * (ring - step) as i32 + bq * step as i32;
    let r = ar * (ring - step) as i32 + br * step as i32;
    (q as f32 + r as f32 * 0.5, r as f32 * 0.866_025_4)
}

fn place_folder(
    folder: &Folder,
    path: &str,
    label: &str,
    root: &Path,
    dependents: &HashMap<String, usize>,
    output: &mut Vec<Bubble>,
    include_self: bool,
    compact: bool,
) -> f32 {
    let start = output.len();
    if include_self {
        output.push(Bubble {
            x: 0.,
            y: 0.,
            radius: 0.,
            label: label.into(),
            path: path.into(),
            change: Change::Unchanged,
            contains_changes: false,
            importance: 0.,
            dependents: 0,
            lines: 0,
            directory: true,
            subtree_end: 0,
        });
    }
    let mut children = Vec::new();
    for (name, subfolder) in &folder.folders {
        let mut child_path = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}/{name}")
        };
        let mut child_label = name.clone();
        let mut child_folder = subfolder;
        if compact {
            while child_folder.files.is_empty() && child_folder.folders.len() == 1 {
                let (next_name, next_folder) = child_folder.folders.iter().next().unwrap();
                child_path.push('/');
                child_path.push_str(next_name);
                child_label.push('/');
                child_label.push_str(next_name);
                child_folder = next_folder;
            }
        }
        let index = output.len();
        let radius = place_folder(
            child_folder,
            &child_path,
            &child_label,
            root,
            dependents,
            output,
            true,
            compact,
        );
        children.push((index, output.len(), radius));
    }
    for file in &folder.files {
        let measured = estimate_lines(root, &file.path);
        let lines = if measured == 0 && file.change == Change::Removed {
            file.churn
        } else {
            measured
        };
        let degree = *dependents.get(&file.path).unwrap_or(&0);
        let (radius, importance) = file_radius(lines, degree, file.churn, file.change);
        let index = output.len();
        output.push(Bubble {
            x: 0.,
            y: 0.,
            radius,
            label: file.path.rsplit('/').next().unwrap_or(&file.path).into(),
            path: file.path.clone(),
            change: file.change,
            contains_changes: file.change != Change::Unchanged,
            importance,
            dependents: degree,
            lines,
            directory: false,
            subtree_end: index + 1,
        });
        children.push((index, index + 1, radius));
    }
    let contains_changes = children
        .iter()
        .any(|(first, _, _)| output[*first].contains_changes);
    // Put the largest groups near the middle, but keep repository order for
    // stable placement across refreshes. Hex rings cost linear time.
    children.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    let largest = children.first().map_or(0., |c| c.2);
    let smallest = children.last().map_or(0., |c| c.2);
    let varied = smallest > 0. && largest / smallest > 1.6;
    let gap = if compact { 6. } else { 8. };
    let cell = largest * 2. + gap;
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut ring_radius = 0_f32;
    let mut ring_outer = largest;
    let mut angle = 0_f32;
    let mut ring_max = 0_f32;
    for (slot, (first, end, radius)) in children.into_iter().enumerate() {
        let (dx, dy) = if varied && slot > 0 {
            let mut half_angle;
            loop {
                if ring_radius == 0. {
                    ring_radius = ring_outer + radius + gap;
                    angle = 0.;
                    ring_max = 0.;
                }
                half_angle = ((radius + gap * 0.5) / ring_radius).min(0.95).asin();
                if angle + 2. * half_angle <= std::f32::consts::TAU || angle == 0. {
                    break;
                }
                ring_outer = ring_radius + ring_max;
                ring_radius = ring_outer + radius + gap;
                angle = 0.;
                ring_max = 0.;
            }
            angle += half_angle;
            let position = (ring_radius * angle.cos(), ring_radius * angle.sin());
            angle += half_angle;
            ring_max = ring_max.max(radius);
            position
        } else {
            let (hx, hy) = hex_position(slot);
            (hx * cell, hy * cell)
        };
        for node in &mut output[first..end] {
            node.x += dx;
            node.y += dy;
        }
        min_x = min_x.min(dx - radius);
        min_y = min_y.min(dy - radius);
        max_x = max_x.max(dx + radius);
        max_y = max_y.max(dy + radius);
    }
    let center_x = if min_x.is_finite() {
        (min_x + max_x) * 0.5
    } else {
        0.
    };
    let center_y = if min_y.is_finite() {
        (min_y + max_y) * 0.5
    } else {
        0.
    };
    let mut extent = 0_f32;
    for node in &mut output[start + usize::from(include_self)..] {
        node.x -= center_x;
        node.y -= center_y;
    }
    // Only direct children determine the enclosing radius. Their descendants
    // already sit inside each child's circle.
    for node in &output[start + usize::from(include_self)..] {
        extent = extent.max(node.x.hypot(node.y) + node.radius);
    }
    let padding = if compact { 8. } else { 18. };
    let radius =
        (extent + if include_self { padding } else { 0. }).max(if compact { 16. } else { 20. });
    if include_self {
        output[start].radius = radius;
        output[start].subtree_end = output.len();
        output[start].contains_changes = contains_changes;
    }
    radius
}
