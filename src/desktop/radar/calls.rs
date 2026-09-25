//! Conservative static call resolution, independently for each snapshot.
use super::{resolve::Resolver, Change, FunctionId, Graph, Snapshot};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn exported(
    snapshot: &Snapshot,
    resolver: &Resolver<'_>,
    path: String,
    name: String,
) -> Option<FunctionId> {
    let mut pending = vec![(path, name)];
    let mut seen = BTreeSet::new();
    let mut candidates = BTreeSet::new();
    while let Some((path, name)) = pending.pop() {
        if !seen.insert((path.clone(), name.clone())) {
            continue;
        }
        let module = snapshot.modules.get(&path)?;
        let (base, suffix) = name
            .split_once('.')
            .map_or((name.as_str(), String::new()), |(a, b)| {
                (a, format!(".{b}"))
            });
        if let Some((source, local)) = module.exports.get(base) {
            if let Some(source) = source {
                if let Some(target) = resolver.resolve(&path, source) {
                    pending.push((target, format!("{local}{suffix}")));
                }
            } else if let Some((source, imported)) = module.imports.get(local) {
                if let Some(target) = resolver.resolve(&path, source) {
                    pending.push((target, format!("{imported}{suffix}")));
                }
            } else {
                let local = format!("{local}{suffix}");
                if module.functions.contains_key(&local) {
                    candidates.insert((path.clone(), local));
                }
            }
        } else if base != "default" {
            for source in &module.export_stars {
                if let Some(target) = resolver.resolve(&path, source) {
                    pending.push((target, name.clone()));
                }
            }
        }
    }
    // Ambiguous star exports cannot establish a call target.
    if candidates.len() == 1 {
        candidates.pop_first()
    } else {
        None
    }
}

pub(super) fn edges(
    snapshot: &Snapshot,
    resolver: &Resolver<'_>,
) -> BTreeSet<(FunctionId, FunctionId)> {
    let mut result = BTreeSet::new();
    for (path, module) in &snapshot.modules {
        for (caller, function) in &module.functions {
            for target in &function.calls {
                let base = target.split('.').next().unwrap_or(target);
                if function.shadows.contains(base) {
                    continue;
                }
                // Look up lexical function scopes before consulting imports.
                let mut scope = caller.as_str();
                let mut local = None;
                loop {
                    let name = if let Some(member) = target.strip_prefix("this.") {
                        scope
                            .rsplit_once('.')
                            .map(|(owner, _)| format!("{owner}.{member}"))
                    } else {
                        Some(if scope.is_empty() {
                            target.clone()
                        } else {
                            format!("{scope}.{target}")
                        })
                    };
                    if let Some(name) = name.filter(|n| module.functions.contains_key(n)) {
                        local = Some((path.clone(), name));
                        break;
                    }
                    if scope.is_empty() {
                        break;
                    }
                    scope = scope.rsplit_once('.').map_or("", |(parent, _)| parent);
                    // Parameters of enclosing named functions also shadow imports.
                    if module
                        .functions
                        .get(scope)
                        .is_some_and(|f| f.shadows.contains(base))
                    {
                        break;
                    }
                }
                let resolved = local.or_else(|| {
                    // Do not resolve an imported name shadowed by an enclosing scope.
                    let mut parent = caller.as_str();
                    while let Some((scope, _)) = parent.rsplit_once('.') {
                        if module
                            .functions
                            .get(scope)
                            .is_some_and(|f| f.shadows.contains(base))
                        {
                            return None;
                        }
                        parent = scope;
                    }
                    let (source, imported) = module.imports.get(base)?;
                    let suffix = target.strip_prefix(base)?;
                    let name = if imported == "*" {
                        suffix.strip_prefix('.')?.to_string()
                    } else {
                        format!("{imported}{suffix}")
                    };
                    exported(snapshot, resolver, resolver.resolve(path, source)?, name)
                });
                if let Some(to) = resolved {
                    result.insert(((path.clone(), caller.clone()), to));
                }
            }
        }
    }
    result
}

pub(super) fn retain_context(graph: &mut Graph) {
    let changes = graph
        .files
        .iter()
        .flat_map(|(path, file)| {
            file.functions
                .iter()
                .map(move |f| ((path.clone(), f.name.clone()), f.change))
        })
        .collect::<BTreeMap<_, _>>();
    let mut seeds = changes
        .iter()
        .filter(|(_, c)| **c != Change::Unchanged)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    // A binding/re-export can change a target without changing the caller's
    // body. Keep that caller and trace its old and new dependencies as context.
    seeds.extend(
        graph
            .calls
            .iter()
            .filter(|e| e.change != Change::Unchanged)
            .map(|e| e.from.clone()),
    );
    let mut relevant = seeds.clone();
    // Trace ancestors and descendants independently. Walking an undirected
    // component would pull in unrelated siblings of an affected caller.
    for before in [true, false] {
        for reverse in [true, false] {
            let mut adjacent: BTreeMap<&FunctionId, Vec<&FunctionId>> = BTreeMap::new();
            for edge in &graph.calls {
                if (before && edge.change == Change::Added)
                    || (!before && edge.change == Change::Removed)
                {
                    continue;
                }
                let (from, to) = if reverse {
                    (&edge.to, &edge.from)
                } else {
                    (&edge.from, &edge.to)
                };
                adjacent.entry(from).or_default().push(to);
            }
            let mut queue = seeds
                .iter()
                .filter(|id| {
                    if before {
                        changes[*id] != Change::Added
                    } else {
                        changes[*id] != Change::Removed
                    }
                })
                .collect::<VecDeque<_>>();
            let mut seen = BTreeSet::new();
            while let Some(id) = queue.pop_front() {
                if !seen.insert(id) {
                    continue;
                }
                relevant.insert(id.clone());
                queue.extend(adjacent.get(id).into_iter().flatten().copied());
            }
        }
    }
    for (path, file) in &mut graph.files {
        file.functions
            .retain(|f| relevant.contains(&(path.clone(), f.name.clone())));
    }
    graph
        .calls
        .retain(|e| relevant.contains(&e.from) && relevant.contains(&e.to));
}
