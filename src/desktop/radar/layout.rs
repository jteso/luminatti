use super::{Change, FunctionNode, Graph};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

const WIDTH: f32 = 248.;
const PAD: f32 = 10.;
const TITLE: f32 = 44.;
const SUMMARY: f32 = 20.;
pub(super) const ROW: f32 = 26.;
const GAP: f32 = 40.;
const MAX_ROW_WIDTH: f32 = 920.;

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
pub(in super::super) struct Position {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug)]
pub(in super::super) struct PlacedFunction {
    pub node: FunctionNode,
    pub position: Position,
    pub width: f32,
    pub depth: usize,
    pub calls: Vec<String>,
}
#[derive(Clone, Debug)]
pub(in super::super) struct Group {
    pub path: String,
    pub file_name: String,
    pub package: String,
    pub change: Change,
    pub root: bool,
    pub entry: bool,
    pub position: Position,
    pub width: f32,
    pub height: f32,
    pub functions: Vec<PlacedFunction>,
    pub counts: [usize; 4],
    pub expanded: bool,
}
#[derive(Clone, Debug)]
pub(in super::super) struct ContextControl {
    pub key: String,
    pub files: Vec<String>,
    pub expanded: bool,
    pub position: Position,
    pub width: f32,
}
#[derive(Clone, Debug)]
pub(in super::super) struct Cycle {
    pub key: String,
    pub count: usize,
    pub expanded: bool,
    pub position: Position,
    pub width: f32,
    pub height: f32,
}
#[derive(Clone, Debug)]
pub(in super::super) struct Connection {
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub points: Vec<Position>,
    pub change: Change,
}
#[derive(Clone, Debug, Default)]
pub(in super::super) struct Diagram {
    pub groups: Vec<Group>,
    pub connections: Vec<Connection>,
    pub context: Vec<ContextControl>,
    pub cycles: Vec<Cycle>,
    pub width: f32,
    pub height: f32,
    pub isolated_files: usize,
    pub skipped_files: usize,
    pub unresolved: usize,
    pub total_chains: usize,
    pub all_expanded: bool,
    pub changed_files: usize,
    pub tala: bool,
}
#[derive(Clone)]
struct Unit {
    members: Vec<String>,
    gap: bool,
    chain: Option<Vec<String>>,
    cycle: bool,
}
fn key(paths: &[String]) -> String {
    paths.join("\0")
}
fn card_width(graph: &Graph, path: &str, expanded_files: &BTreeSet<String>) -> f32 {
    let file = &graph.files[path];
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);
    let mut width = name.chars().count() as f32 * 7.5 + 46.;
    if expanded_files.contains(path) {
        for function in &file.functions {
            width = width.max(function.name.chars().count() as f32 * 7. + 90.);
        }
    }
    width.clamp(WIDTH, 360.).ceil()
}
fn height(graph: &Graph, path: &str, expanded_files: &BTreeSet<String>) -> f32 {
    let count = graph.files[path].functions.len();
    TITLE
        + if count > 0 { SUMMARY } else { 0. }
        + if expanded_files.contains(path) {
            count as f32 * ROW
        } else {
            0.
        }
        + 6.
}
fn adjacency(
    size: usize,
    edges: &BTreeMap<(usize, usize), Change>,
) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let mut outgoing = vec![Vec::new(); size];
    let mut incoming = vec![Vec::new(); size];
    for &(from, to) in edges.keys() {
        outgoing[from].push(to);
        incoming[to].push(from);
    }
    (outgoing, incoming)
}
// Iterative Kosaraju: deep import chains and cycles cannot overflow the stack.
fn components(outgoing: &[Vec<usize>], incoming: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut seen = vec![false; outgoing.len()];
    let mut order = Vec::new();
    for start in 0..outgoing.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![(start, 0)];
        while let Some((node, index)) = stack.last_mut() {
            if *index < outgoing[*node].len() {
                let next = outgoing[*node][*index];
                *index += 1;
                if !seen[next] {
                    seen[next] = true;
                    stack.push((next, 0));
                }
            } else {
                order.push(*node);
                stack.pop();
            }
        }
    }
    seen.fill(false);
    let mut result = Vec::new();
    for start in order.into_iter().rev() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut part = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            part.push(node);
            for &next in &incoming[node] {
                if !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        part.sort();
        result.push(part);
    }
    result.sort_by_key(|part| part[0]);
    result
}
fn combine(a: Change, b: Change) -> Change {
    if a == b {
        a
    } else {
        Change::Modified
    }
}
fn remap(
    edges: &BTreeMap<(usize, usize), Change>,
    mapping: &[usize],
) -> BTreeMap<(usize, usize), Change> {
    let mut result = BTreeMap::new();
    for (&(from, to), &change) in edges {
        let pair = (mapping[from], mapping[to]);
        if pair.0 != pair.1 {
            result
                .entry(pair)
                .and_modify(|value| *value = combine(*value, change))
                .or_insert(change);
        }
    }
    result
}

fn place_functions(graph: &Graph, path: &str, x: f32, y: f32, width: f32) -> Vec<PlacedFunction> {
    let functions = &graph.files[path].functions;
    let indices = functions
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    let edges = graph
        .calls
        .iter()
        .filter(|e| e.from.0 == path && e.to.0 == path)
        .filter_map(|e| {
            Some((
                (
                    *indices.get(e.from.1.as_str())?,
                    *indices.get(e.to.1.as_str())?,
                ),
                e.change,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let (outgoing, incoming) = adjacency(functions.len(), &edges);
    let parts = components(&outgoing, &incoming);
    let mut mapping = vec![0; functions.len()];
    for (i, part) in parts.iter().enumerate() {
        for &node in part {
            mapping[node] = i;
        }
    }
    let (outgoing, incoming) = adjacency(parts.len(), &remap(&edges, &mapping));
    let mut degrees = incoming.iter().map(Vec::len).collect::<Vec<_>>();
    let mut depths = vec![0; parts.len()];
    let mut queue = (0..parts.len())
        .filter(|i| degrees[*i] == 0)
        .collect::<VecDeque<_>>();
    while let Some(node) = queue.pop_front() {
        for &next in &outgoing[node] {
            depths[next] = depths[next].max(depths[node] + 1);
            degrees[next] -= 1;
            if degrees[next] == 0 {
                queue.push_back(next);
            }
        }
    }
    let mut order = (0..functions.len()).collect::<Vec<_>>();
    order.sort_by_key(|i| (depths[mapping[*i]], functions[*i].line));
    order
        .into_iter()
        .enumerate()
        .map(|(row, i)| {
            let depth = depths[mapping[i]];
            let indent = depth.min(4) as f32 * 10.;
            PlacedFunction {
                node: functions[i].clone(),
                position: Position {
                    x: x + PAD + indent,
                    y: y + TITLE + SUMMARY + row as f32 * ROW,
                },
                width: width - 2. * PAD - indent,
                depth,
                calls: graph
                    .calls
                    .iter()
                    .filter(|e| e.from.0 == path && e.from.1 == functions[i].name)
                    .map(|e| format!("{} {}() — {}", e.change.marker(), e.to.1, e.to.0))
                    .collect(),
            }
        })
        .collect()
}
impl Diagram {
    #[cfg(test)]
    pub fn new(graph: &Graph) -> Self {
        Self::with_options(graph, false, &BTreeSet::new(), &BTreeSet::new())
    }
    #[cfg(test)]
    pub fn with_options(
        graph: &Graph,
        expand_all: bool,
        expanded: &BTreeSet<String>,
        expanded_cycles: &BTreeSet<String>,
    ) -> Self {
        Self::with_files(
            graph,
            expand_all,
            expanded,
            expanded_cycles,
            &BTreeSet::new(),
        )
    }
    pub fn with_files(
        graph: &Graph,
        expand_all: bool,
        expanded: &BTreeSet<String>,
        expanded_cycles: &BTreeSet<String>,
        expanded_files: &BTreeSet<String>,
    ) -> Self {
        Self::build(
            graph,
            expand_all,
            expanded,
            expanded_cycles,
            expanded_files,
            false,
        )
    }
    pub(super) fn with_tala(
        graph: &Graph,
        expand_all: bool,
        expanded: &BTreeSet<String>,
        expanded_cycles: &BTreeSet<String>,
        expanded_files: &BTreeSet<String>,
    ) -> Self {
        Self::build(
            graph,
            expand_all,
            expanded,
            expanded_cycles,
            expanded_files,
            true,
        )
    }
    fn build(
        graph: &Graph,
        expand_all: bool,
        expanded: &BTreeSet<String>,
        expanded_cycles: &BTreeSet<String>,
        expanded_files: &BTreeSet<String>,
        use_tala: bool,
    ) -> Self {
        let mut diagram = Self {
            skipped_files: graph.skipped_files,
            unresolved: graph.unresolved,
            ..Self::default()
        };
        let connected = graph
            .edges
            .iter()
            .flat_map(|e| [&e.from, &e.to])
            .chain(graph.calls.iter().flat_map(|e| [&e.from.0, &e.to.0]))
            .collect::<BTreeSet<_>>();
        let seeds = graph
            .files
            .values()
            .filter(|f| f.change != Change::Unchanged && (connected.contains(&f.path) || f.entry))
            .map(|f| f.path.clone())
            .collect::<BTreeSet<_>>();
        diagram.isolated_files = graph
            .files
            .values()
            .filter(|f| f.change != Change::Unchanged && !seeds.contains(&f.path))
            .count();
        diagram.changed_files = seeds.len();
        // Trace each snapshot separately: never stitch an old-only prefix to a
        // new-only suffix and claim it is a transitive dependency path.
        let mut relevant = seeds.clone();
        relevant.extend(
            graph
                .files
                .values()
                .filter(|f| f.functions.iter().any(|n| n.change == Change::Unchanged))
                .map(|f| f.path.clone()),
        );
        let function_context = relevant
            .difference(&seeds)
            .cloned()
            .collect::<BTreeSet<_>>();
        for before in [true, false] {
            let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            let mut forward: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for edge in &graph.edges {
                if (before && edge.change == Change::Added)
                    || (!before && edge.change == Change::Removed)
                {
                    continue;
                }
                reverse.entry(&edge.to).or_default().push(&edge.from);
                forward.entry(&edge.from).or_default().push(&edge.to);
            }
            let mut visited = BTreeSet::new();
            let mut queue = seeds
                .iter()
                .filter(|p| {
                    if before {
                        graph.files[*p].change != Change::Added
                    } else {
                        graph.files[*p].change != Change::Removed
                    }
                })
                .map(String::as_str)
                .collect::<VecDeque<_>>();
            while let Some(path) = queue.pop_front() {
                if !visited.insert(path) {
                    continue;
                }
                relevant.insert(path.into());
                for parent in reverse.get(path).into_iter().flatten() {
                    queue.push_back(parent);
                }
            }
            // Preserve import/re-export bridges to transitive callees without
            // pulling in every unrelated importer of a shared utility.
            let mut descendants = BTreeSet::new();
            let mut queue = seeds
                .iter()
                .filter(|p| {
                    if before {
                        graph.files[*p].change != Change::Added
                    } else {
                        graph.files[*p].change != Change::Removed
                    }
                })
                .map(String::as_str)
                .collect::<VecDeque<_>>();
            while let Some(path) = queue.pop_front() {
                if !descendants.insert(path) {
                    continue;
                }
                queue.extend(forward.get(path).into_iter().flatten().copied());
            }
            let mut queue = function_context
                .iter()
                .map(String::as_str)
                .collect::<VecDeque<_>>();
            let mut visited = BTreeSet::new();
            while let Some(path) = queue.pop_front() {
                if !descendants.contains(path) || !visited.insert(path) {
                    continue;
                }
                relevant.insert(path.into());
                queue.extend(reverse.get(path).into_iter().flatten().copied());
            }
        }
        let paths = relevant.into_iter().collect::<Vec<_>>();
        if paths.is_empty() {
            return diagram;
        }
        let indices = paths
            .iter()
            .enumerate()
            .map(|(i, p)| (p.as_str(), i))
            .collect::<BTreeMap<_, _>>();
        let raw_edges = graph
            .edges
            .iter()
            .filter_map(|e| {
                Some((
                    (*indices.get(e.from.as_str())?, *indices.get(e.to.as_str())?),
                    e.change,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let (outgoing, incoming) = adjacency(paths.len(), &raw_edges);
        let scc = components(&outgoing, &incoming);
        let mut mapping = vec![0; paths.len()];
        let mut units = Vec::new();
        for (i, part) in scc.iter().enumerate() {
            for &node in part {
                mapping[node] = i;
            }
            units.push(Unit {
                members: part.iter().map(|i| paths[*i].clone()).collect(),
                gap: false,
                chain: None,
                cycle: part.len() > 1,
            });
        }
        let edges = remap(&raw_edges, &mapping);
        let (outgoing, incoming) = adjacency(units.len(), &edges);
        let collapsible = (0..units.len())
            .map(|i| {
                !units[i].cycle
                    && !graph.files[&units[i].members[0]].entry
                    && graph.files[&units[i].members[0]].change == Change::Unchanged
                    && incoming[i].len() == 1
                    && outgoing[i].len() == 1
                    && edges[&(incoming[i][0], i)] == Change::Unchanged
                    && edges[&(i, outgoing[i][0])] == Change::Unchanged
            })
            .collect::<Vec<_>>();
        let mut chains = Vec::new();
        for i in 0..units.len() {
            if !collapsible[i] || collapsible[incoming[i][0]] {
                continue;
            }
            let mut chain = vec![i];
            let mut next = outgoing[i][0];
            while collapsible[next] {
                chain.push(next);
                next = outgoing[next][0];
            }
            chains.push(chain);
        }
        diagram.total_chains = chains.len();
        let mut replacements = BTreeMap::new();
        let mut removed = BTreeSet::new();
        let mut opened = 0;
        for chain in chains {
            let files = chain
                .iter()
                .map(|i| units[*i].members[0].clone())
                .collect::<Vec<_>>();
            let is_expanded = expand_all ^ expanded.contains(&key(&files));
            if is_expanded {
                opened += 1;
                units[*chain.last().unwrap()].chain = Some(files);
            } else {
                units[chain[0]].members = files;
                units[chain[0]].gap = true;
                for &i in &chain[1..] {
                    removed.insert(i);
                    replacements.insert(i, chain[0]);
                }
            }
        }
        diagram.all_expanded = opened == diagram.total_chains;
        let mut new_mapping = vec![0; units.len()];
        let mut visible = Vec::new();
        for (i, unit) in units.into_iter().enumerate() {
            if !removed.contains(&i) {
                new_mapping[i] = visible.len();
                visible.push(unit);
            }
        }
        for (i, target) in replacements {
            new_mapping[i] = new_mapping[target];
        }
        let edges = remap(&edges, &new_mapping);
        let units = visible;
        let (outgoing, incoming) = adjacency(units.len(), &edges);
        // Weak components occupy independent columns; shared dependencies put
        // their entry points in the same component and are drawn only once.
        let mut seen = BTreeSet::new();
        let mut forest = Vec::new();
        for i in 0..units.len() {
            if !seen.insert(i) {
                continue;
            }
            let mut tree = Vec::new();
            let mut queue = VecDeque::from([i]);
            while let Some(node) = queue.pop_front() {
                tree.push(node);
                for &next in outgoing[node].iter().chain(&incoming[node]) {
                    if seen.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
            tree.sort();
            forest.push(tree);
        }
        // Order trees by their roots, not by whichever descendant happens to
        // sort first. Adding a leaf should not move an independent tree.
        forest.sort_by_cached_key(|tree| {
            let roots = tree
                .iter()
                .filter(|i| incoming[**i].is_empty())
                .flat_map(|i| units[*i].members.iter())
                .collect::<Vec<_>>();
            (
                !roots.iter().any(|path| graph.files[*path].entry),
                roots.into_iter().min().cloned().unwrap_or_default(),
            )
        });
        let mut sizes = Vec::new();
        for unit in &units {
            let expanded = expanded_cycles.contains(&key(&unit.members));
            let internal = if unit.cycle && expanded {
                graph
                    .edges
                    .iter()
                    .filter(|e| unit.members.contains(&e.from) && unit.members.contains(&e.to))
                    .count()
            } else {
                0
            };
            let width = unit
                .members
                .iter()
                .map(|p| card_width(graph, p, expanded_files))
                .fold(WIDTH, f32::max)
                + if unit.cycle {
                    24. + internal as f32 * 6.
                } else {
                    0.
                };
            let h = if unit.gap {
                28.
            } else {
                unit.members
                    .iter()
                    .map(|p| height(graph, p, expanded_files))
                    .sum::<f32>()
                    + if unit.cycle {
                        30. + (unit.members.len() - 1) as f32 * if expanded { 28. } else { 6. } + 8.
                    } else {
                        0.
                    }
                    + if unit.chain.is_some() { 28. } else { 0. }
            };
            sizes.push((width, h));
        }
        let mut positions = vec![Position::default(); units.len()];
        let mut ranks = vec![0; units.len()];
        let mut exits = vec![0.; units.len()];
        let mut x_offset = 20.;
        let mut y_offset = 20.;
        let mut shelf_bottom = 20_f32;
        let mut indegrees = incoming.iter().map(Vec::len).collect::<Vec<_>>();
        let edge_pairs = edges.keys().copied().collect::<Vec<_>>();
        let geometry = use_tala
            .then(|| super::tala::layout(&sizes, &edge_pairs))
            .flatten();
        if let Some(geometry) = geometry {
            positions = geometry.positions;
            let mut min_x = positions.iter().map(|p| p.x).fold(0_f32, f32::min);
            let mut min_y = positions.iter().map(|p| p.y).fold(0_f32, f32::min);
            for p in geometry.routes.iter().flatten() {
                min_x = min_x.min(p.x);
                min_y = min_y.min(p.y);
            }
            let offset = Position {
                x: 28. - min_x,
                y: 28. - min_y,
            };
            for (i, p) in positions.iter_mut().enumerate() {
                p.x += offset.x;
                p.y += offset.y;
                diagram.width = diagram.width.max(p.x + sizes[i].0 + 28.);
                diagram.height = diagram.height.max(p.y + sizes[i].1 + 28.);
            }
            for ((from, to), mut points) in edge_pairs.into_iter().zip(geometry.routes) {
                for p in &mut points {
                    p.x += offset.x;
                    p.y += offset.y;
                    diagram.width = diagram.width.max(p.x + 28.);
                    diagram.height = diagram.height.max(p.y + 28.);
                }
                diagram.connections.push(Connection {
                    from: units[from].members.clone(),
                    to: units[to].members.clone(),
                    points,
                    change: edges[&(from, to)],
                });
            }
            diagram.tala = true;
        } else {
            for tree in forest {
                let mut queue = tree
                    .iter()
                    .copied()
                    .filter(|i| indegrees[*i] == 0)
                    .collect::<VecDeque<_>>();
                while let Some(node) = queue.pop_front() {
                    for &next in &outgoing[node] {
                        ranks[next] = ranks[next].max(ranks[node] + 1);
                        indegrees[next] -= 1;
                        if indegrees[next] == 0 {
                            queue.push_back(next);
                        }
                    }
                }
                let mut levels: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
                for &i in &tree {
                    levels.entry(ranks[i]).or_default().push(i);
                }
                let width = levels
                    .values()
                    .map(|level| {
                        level.iter().map(|i| sizes[*i].0).sum::<f32>()
                            + (level.len() - 1) as f32 * 24.
                    })
                    .fold(0_f32, f32::max)
                    .min(MAX_ROW_WIDTH)
                    .max(tree.iter().map(|i| sizes[*i].0).fold(0_f32, f32::max));
                if x_offset > 20. && x_offset + width + 76. > MAX_ROW_WIDTH + 100. {
                    x_offset = 20.;
                    y_offset = shelf_bottom + GAP;
                }
                let mut y = y_offset;
                for level in levels.values_mut() {
                    let anchor = |i: usize| {
                        if incoming[i].is_empty() {
                            i as f32
                        } else {
                            incoming[i]
                                .iter()
                                .map(|p| positions[*p].x + sizes[*p].0 / 2.)
                                .sum::<f32>()
                                / incoming[i].len() as f32
                        }
                    };
                    level.sort_by(|a, b| anchor(*a).total_cmp(&anchor(*b)).then(a.cmp(b)));
                    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
                    let mut row_width = 0.;
                    for &i in level.iter() {
                        if row_width > 0. && row_width + sizes[i].0 > width {
                            rows.push(Vec::new());
                            row_width = 0.;
                        }
                        rows.last_mut().unwrap().push(i);
                        row_width += sizes[i].0 + 24.;
                    }
                    // Wrap siblings inside their rank. Finish the entire rank
                    // before placing any dependency, even after card expansion.
                    for row in rows {
                        let row_width = row.iter().map(|i| sizes[*i].0).sum::<f32>()
                            + (row.len() - 1) as f32 * 24.;
                        let mut x = x_offset + (width - row_width) / 2.;
                        let tallest = row.iter().map(|i| sizes[*i].1).fold(0_f32, f32::max);
                        for i in row {
                            positions[i] = Position { x, y };
                            x += sizes[i].0 + 24.;
                            exits[i] = y + tallest + 12.;
                        }
                        y += tallest + GAP;
                    }
                }
                for (from, to) in tree
                    .iter()
                    .flat_map(|from| outgoing[*from].iter().map(move |to| (*from, *to)))
                {
                    let change = edges[&(from, to)];
                    let a = Position {
                        x: positions[from].x + sizes[from].0 / 2.,
                        y: positions[from].y + sizes[from].1,
                    };
                    let b = Position {
                        x: positions[to].x + sizes[to].0 / 2.,
                        y: positions[to].y,
                    };
                    let points = if ranks[to] > ranks[from] + 1 || exits[from] < b.y - GAP + 12. {
                        // Route past wrapped rows outside the cards. Reuse a
                        // bounded set of lanes so dense test roots cannot widen
                        // the canvas by thousands of pixels.
                        let rail = x_offset + width + 12. + (from % 8) as f32 * 8.;
                        vec![
                            a,
                            Position {
                                x: a.x,
                                y: exits[from],
                            },
                            Position {
                                x: rail,
                                y: exits[from],
                            },
                            Position {
                                x: rail,
                                y: b.y - 12.,
                            },
                            Position {
                                x: b.x,
                                y: b.y - 12.,
                            },
                            b,
                        ]
                    } else {
                        let via = b.y - GAP / 2.;
                        vec![
                            a,
                            Position { x: a.x, y: via },
                            Position { x: b.x, y: via },
                            b,
                        ]
                    };
                    diagram.connections.push(Connection {
                        from: units[from].members.clone(),
                        to: units[to].members.clone(),
                        points,
                        change,
                    });
                }
                x_offset += width + 100.;
                shelf_bottom = shelf_bottom.max(y - GAP);
                diagram.width = diagram.width.max(x_offset - 20.);
                diagram.height = shelf_bottom + 20.;
            }
        }
        for (i, unit) in units.iter().enumerate() {
            let position = positions[i];
            let (width, h) = sizes[i];
            if unit.gap {
                diagram.context.push(ContextControl {
                    key: key(&unit.members),
                    files: unit.members.clone(),
                    expanded: false,
                    position,
                    width,
                });
                continue;
            }
            let expanded = expanded_cycles.contains(&key(&unit.members));
            if unit.cycle {
                diagram.cycles.push(Cycle {
                    key: key(&unit.members),
                    count: unit.members.len(),
                    expanded,
                    position,
                    width,
                    height: h,
                });
            }
            let mut y = position.y + if unit.cycle { 30. } else { 0. };
            let mut locations = BTreeMap::new();
            for path in &unit.members {
                let file = &graph.files[path];
                let x = position.x + if unit.cycle { 8. } else { 0. };
                let h = height(graph, path, expanded_files);
                let mut counts = [0; 4];
                for function in &file.functions {
                    counts[match function.change {
                        Change::Added => 0,
                        Change::Modified => 1,
                        Change::Removed => 2,
                        Change::Unchanged => 3,
                    }] += 1;
                }
                let card_width = card_width(graph, path, expanded_files);
                locations.insert(path, (x + card_width, y + h / 2.));
                diagram.groups.push(Group {
                    path: path.clone(),
                    file_name: Path::new(path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(path)
                        .into(),
                    package: file.package.clone(),
                    change: file.change,
                    entry: file.entry,
                    root: incoming[i].is_empty(),
                    position: Position { x, y },
                    width: card_width,
                    height: h,
                    counts,
                    expanded: expanded_files.contains(path),
                    functions: if expanded_files.contains(path) {
                        place_functions(graph, path, x, y, card_width)
                    } else {
                        Vec::new()
                    },
                });
                y += h + if unit.cycle && expanded { 28. } else { 6. };
            }
            if let Some(files) = &unit.chain {
                diagram.context.push(ContextControl {
                    key: key(files),
                    files: files.clone(),
                    expanded: true,
                    position: Position {
                        x: position.x,
                        y: position.y + h - 24.,
                    },
                    width,
                });
            }
            if unit.cycle && expanded {
                for (rail, edge) in graph
                    .edges
                    .iter()
                    .filter(|e| unit.members.contains(&e.from) && unit.members.contains(&e.to))
                    .enumerate()
                {
                    let &(ax, ay) = &locations[&edge.from];
                    let &(bx, by) = &locations[&edge.to];
                    let x = position.x + width - 8. - rail as f32 * 6.;
                    diagram.connections.push(Connection {
                        from: vec![edge.from.clone()],
                        to: vec![edge.to.clone()],
                        points: vec![
                            Position { x: ax, y: ay },
                            Position { x, y: ay },
                            Position { x, y: by },
                            Position { x: bx, y: by },
                        ],
                        change: edge.change,
                    });
                }
            }
        }
        diagram
    }
}
