use super::*;

fn snapshot(files: &[(&str, &str)]) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for (path, source) in files {
        insert(&mut snapshot, path, source.as_bytes());
    }
    snapshot
}
fn group<'a>(diagram: &'a Diagram, path: &str) -> &'a layout::Group {
    diagram.groups.iter().find(|g| g.path == path).unwrap()
}
fn chain() -> Graph {
    compare(
        snapshot(&[
            ("main.ts", "import './a';"),
            ("a.ts", "import './b'; export function a() { return 1; }"),
            ("b.ts", "export * from './c';"),
            ("c.ts", "import type { Value } from './d';"),
            (
                "d.ts",
                "export type Value = string; export function d() { return 1; }",
            ),
            ("unused.ts", "export const unused = 1;"),
        ]),
        snapshot(&[
            ("main.ts", "import './a';"),
            ("a.ts", "import './b'; export function a() { return 2; }"),
            ("b.ts", "export * from './c';"),
            ("c.ts", "import type { Value } from './d';"),
            (
                "d.ts",
                "export type Value = string; export function d() { return 2; }",
            ),
            ("unused.ts", "export const unused = 2;"),
        ]),
    )
}

#[test]
fn dependency_focus_includes_transitive_imports_and_parent_paths_to_roots() {
    let graph = chain();
    let focus = dependency_focus(&graph, "a.ts").unwrap();
    assert_eq!(
        focus.files,
        BTreeSet::from([
            "a.ts".into(),
            "b.ts".into(),
            "c.ts".into(),
            "d.ts".into(),
            "main.ts".into(),
        ])
    );
    assert_eq!(
        focus.edges,
        BTreeSet::from([
            ("a.ts".into(), "b.ts".into()),
            ("b.ts".into(), "c.ts".into()),
            ("c.ts".into(), "d.ts".into()),
            ("main.ts".into(), "a.ts".into()),
        ])
    );
    assert!(!focus.files.contains("unused.ts"));
}

#[test]
fn dependency_focus_does_not_include_sibling_branches_from_a_parent() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[
            ("root.ts", "import './selected'; import './sibling';"),
            ("selected.ts", "import './leaf';"),
            ("sibling.ts", "export const sibling = 1;"),
            ("leaf.ts", "export const leaf = 1;"),
        ]),
    );
    let focus = dependency_focus(&graph, "selected.ts").unwrap();
    assert_eq!(
        focus.files,
        BTreeSet::from(["root.ts".into(), "selected.ts".into(), "leaf.ts".into()])
    );
    assert!(!focus.files.contains("sibling.ts"));
}

#[test]
fn dependency_focus_supports_multiple_files_and_clears_hidden_cards() {
    let graph = chain();
    let mut state = RadarState::default();
    state.graph = Some(Arc::new(graph));
    state.relayout();

    state.toggle_dependency_focus("a.ts".into());
    state.toggle_dependency_focus("d.ts".into());
    assert_eq!(
        state.focused_files,
        BTreeSet::from(["a.ts".into(), "d.ts".into()])
    );
    assert_eq!(state.dependency_focus().unwrap().files.len(), 5);
    state.toggle_dependency_focus("a.ts".into());
    assert_eq!(state.focused_files, BTreeSet::from(["d.ts".into()]));

    state.focused_files.insert("b.ts".into());
    state.relayout();
    assert_eq!(state.focused_files, BTreeSet::from(["d.ts".into()]));

    state.clear_dependency_focus();
    assert!(state.focused_files.is_empty());
}
#[test]
fn transitive_context_collapses_and_expands_without_losing_changed_files() {
    let graph = chain();
    let collapsed = Diagram::new(&graph);
    assert_eq!(graph.edges.len(), 4);
    assert_eq!(collapsed.groups.len(), 3);
    assert_eq!(collapsed.context.len(), 1);
    assert_eq!(collapsed.context[0].files, vec!["b.ts", "c.ts"]);
    assert_eq!(collapsed.isolated_files, 1);
    assert_eq!(group(&collapsed, "a.ts").counts, [0, 1, 0, 0]);
    assert!(group(&collapsed, "a.ts").functions.is_empty());
    assert!(group(&collapsed, "main.ts").functions.is_empty());
    assert!(group(&collapsed, "main.ts").root);
    let expanded = Diagram::with_options(
        &graph,
        false,
        &BTreeSet::from([collapsed.context[0].key.clone()]),
        &BTreeSet::new(),
    );
    assert_eq!(expanded.groups.len(), 5);
    assert!(expanded.all_expanded);
    for file in ["b.ts", "c.ts"] {
        assert_eq!(group(&expanded, file).change, Change::Unchanged);
    }
    assert_eq!(expanded.connections.len(), 4);
    assert!(group(&expanded, "d.ts").position.y > group(&expanded, "c.ts").position.y);
}
#[test]
fn filters_remove_files_and_edges() {
    let mut graph = chain();
    filter_graph(&mut graph, &["unused.ts".into(), "c.ts".into()]);
    assert!(!graph.files.contains_key("c.ts"));
    assert!(graph
        .edges
        .iter()
        .all(|e| e.from != "c.ts" && e.to != "c.ts"));
    assert!(!graph.files.contains_key("unused.ts"));
}
#[test]
fn branches_without_changes_are_pruned_and_junctions_stay_visible() {
    let old = [
        ("root.ts", "import './a'; import './b'; import './unused';"),
        ("a.ts", "import './shared';"),
        ("b.ts", "import './shared';"),
        ("shared.ts", "export const v = 1;"),
        ("unused.ts", "export const z = 0;"),
    ];
    let mut new = old;
    new[3].1 = "export const v = 2;";
    let graph = compare(snapshot(&old), snapshot(&new));
    let diagram = Diagram::new(&graph);
    assert!(!diagram.groups.iter().any(|g| g.path == "unused.ts"));
    assert!(group(&diagram, "root.ts").root);
    assert_eq!(diagram.context.len(), 2);
    assert_eq!(diagram.connections.len(), 4);
    assert_eq!(
        diagram
            .groups
            .iter()
            .filter(|g| g.path == "shared.ts")
            .count(),
        1
    );
}
#[test]
fn shared_dependencies_are_once_below_all_parents_and_independent_trees_are_to_the_right() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[
            ("a.ts", "import './shared'; function a() {}"),
            ("b.ts", "import './shared'; function b() {}"),
            ("shared.ts", "export const shared = 1;"),
            ("z.ts", "import './zz';"),
            ("zz.ts", "export const z = 1;"),
        ]),
    );
    let diagram = Diagram::new(&graph);
    let shared = group(&diagram, "shared.ts");
    for path in ["a.ts", "b.ts"] {
        let p = group(&diagram, path);
        assert!(shared.position.y > p.position.y + p.height);
    }
    assert!(group(&diagram, "z.ts").position.x > group(&diagram, "b.ts").position.x);
    assert_eq!(diagram.groups.len(), 5);
    assert_eq!(diagram.connections.len(), 3);
}
#[test]
fn cycles_preserve_files_and_expand_internal_edges() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[
            ("root.ts", "import './a';"),
            ("a.ts", "import './b'; export function a() {}"),
            ("b.ts", "import './a'; import './tail';"),
            ("tail.ts", "export const tail = 1;"),
        ]),
    );
    let compact = Diagram::new(&graph);
    assert_eq!(compact.cycles.len(), 1);
    assert_eq!(compact.cycles[0].count, 2);
    assert_eq!(compact.groups.len(), 4);
    assert_eq!(compact.connections.len(), 2);
    let expanded = Diagram::with_options(
        &graph,
        false,
        &BTreeSet::new(),
        &BTreeSet::from([compact.cycles[0].key.clone()]),
    );
    assert_eq!(expanded.groups.len(), 4);
    assert_eq!(expanded.connections.len(), 4);
    assert!(expanded.cycles[0].height > compact.cycles[0].height);
    assert!(expanded.width.is_finite() && expanded.height.is_finite());
}
#[test]
fn rootless_cycles_have_a_stable_group() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[("a.ts", "import './b';"), ("b.ts", "import './a';")]),
    );
    let diagram = Diagram::new(&graph);
    assert_eq!(diagram.cycles.len(), 1);
    assert_eq!(diagram.groups.len(), 2);
    assert_eq!(diagram.isolated_files, 0);
}
#[test]
fn removed_imports_keep_historical_context_and_are_not_collapsed() {
    let graph = compare(
        snapshot(&[
            ("root.ts", "import './a';"),
            ("a.ts", "import './b';"),
            ("b.ts", "export const b = 1;"),
        ]),
        snapshot(&[
            ("root.ts", "import './a';"),
            ("a.ts", "export const a = 1;"),
        ]),
    );
    let diagram = Diagram::new(&graph);
    assert_eq!(group(&diagram, "b.ts").change, Change::Removed);
    assert!(diagram
        .connections
        .iter()
        .any(|e| e.change == Change::Removed));
    assert!(diagram.context.is_empty());
}
#[test]
fn before_and_after_paths_are_not_stitched_during_ancestor_selection() {
    let graph = Graph {
        files: [
            ("old-root.ts", Change::Unchanged),
            ("bridge.ts", Change::Unchanged),
            ("leaf.ts", Change::Modified),
        ]
        .into_iter()
        .map(|(p, c)| {
            (
                p.into(),
                FileNode {
                    path: p.into(),
                    package: "repo".into(),
                    change: c,
                    functions: vec![],
                    entry: false,
                },
            )
        })
        .collect(),
        edges: vec![
            Edge {
                from: "old-root.ts".into(),
                to: "bridge.ts".into(),
                change: Change::Removed,
            },
            Edge {
                from: "bridge.ts".into(),
                to: "leaf.ts".into(),
                change: Change::Added,
            },
        ],
        ..Graph::default()
    };
    let diagram = Diagram::new(&graph);
    assert!(!diagram.groups.iter().any(|g| g.path == "old-root.ts"));
    assert_eq!(diagram.groups.len(), 2);
}
#[test]
fn no_changes_means_no_diagram_and_isolated_changes_are_counted() {
    let files = [
        ("main.ts", "import './a';"),
        ("a.ts", "export const a = 1;"),
    ];
    assert!(Diagram::new(&compare(snapshot(&files), snapshot(&files)))
        .groups
        .is_empty());
    let diagram = Diagram::new(&compare(
        Snapshot::default(),
        snapshot(&[("alone.ts", "function a() {}")]),
    ));
    assert!(diagram.groups.is_empty());
    assert_eq!(diagram.isolated_files, 1);
}
#[test]
fn package_entry_points_can_stand_alone() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[
            (
                "package.json",
                r#"{"name":"@app/api","source":"./main.ts"}"#,
            ),
            ("main.ts", "export function start() {}"),
        ]),
    );
    let diagram = Diagram::new(&graph);
    assert_eq!(group(&diagram, "main.ts").package, "@app/api");
    assert!(group(&diagram, "main.ts").entry);
}
#[test]
fn resolves_relative_side_effect_type_reexport_dynamic_and_javascript_imports() {
    let graph=compare(Snapshot::default(),snapshot(&[
        ("main.ts","import './side'; import type { T } from './types'; export * from './barrel'; const load = () => import('./lazy.js');"),
        ("side.ts","export {};"),("types.ts","export type T = string;"),("barrel.ts","export * from './impl';"),
        ("impl.ts","export const value = 1;"),("lazy.js","export const load = () => 1;")]));
    assert_eq!(graph.edges.len(), 5);
    assert_eq!(graph.unresolved, 0);
    assert!(graph
        .edges
        .iter()
        .any(|e| e.from == "main.ts" && e.to == "barrel.ts"));
    assert!(graph
        .edges
        .iter()
        .any(|e| e.from == "barrel.ts" && e.to == "impl.ts"));
}
#[test]
fn resolves_inherited_jsonc_aliases_and_workspace_packages_from_each_snapshot() {
    let graph=compare(Snapshot::default(),snapshot(&[
        ("tsconfig.base.json","{ // config\n \"compilerOptions\": {\"baseUrl\":\".\",\"paths\":{\"@core/*\":[\"packages/core/src/*\",],},},}"),
        ("apps/api/tsconfig.json",r#"{"extends":"../../tsconfig.base.json"}"#),
        ("apps/api/package.json",r#"{"name":"@app/api","source":"./main.ts"}"#),
        ("apps/api/main.ts","import { read } from '@core/read'; import { send } from '@app/mail';"),
        ("packages/core/src/read.ts","export function read() {}"),
        ("packages/mail/package.json",r#"{"name":"@app/mail","exports":{".":{"import":"./src/index.ts"}}}"#),
        ("packages/mail/src/index.ts","export function send() {}") ]));
    assert_eq!(graph.edges.len(), 2);
    assert_eq!(graph.unresolved, 0);
    assert_eq!(
        graph.files["packages/mail/src/index.ts"].package,
        "@app/mail"
    );
    assert!(graph.files["apps/api/main.ts"].entry);
}
#[test]
fn external_and_dynamic_imports_are_reported_without_name_guessing() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[
            (
                "a.ts",
                "import { run } from 'unknown'; const f = () => import(name);",
            ),
            ("b.ts", "export function run() {}"),
        ]),
    );
    assert!(graph.edges.is_empty());
    assert_eq!(graph.unresolved, 2);
}
#[test]
fn syntax_failure_does_not_manufacture_deletions() {
    let graph = compare(
        snapshot(&[
            ("root.ts", "import './a';"),
            ("a.ts", "export function a() {}"),
        ]),
        snapshot(&[
            ("root.ts", "import './a';"),
            ("a.ts", "export function a( {"),
        ]),
    );
    assert!(graph.edges.is_empty());
    assert!(!graph.files.contains_key("a.ts"));
    assert_eq!(graph.files["root.ts"].change, Change::Unchanged);
    assert_eq!(graph.skipped_files, 1);
    assert!(Diagram::new(&graph).groups.is_empty());
}
#[test]
fn file_changes_include_non_function_edits_but_only_changed_functions_are_listed() {
    let old="import './dep'; export const value = 1; function stable() {} function edit() { return 'a b'; }";
    let new="import './dep'; export const value = 2; function stable() {} function edit() { return 'a  b'; }";
    let graph = compare(
        snapshot(&[("a.ts", old), ("dep.ts", "export {};")]),
        snapshot(&[("a.ts", new), ("dep.ts", "export {};")]),
    );
    assert_eq!(graph.files["a.ts"].change, Change::Modified);
    assert_eq!(graph.files["a.ts"].functions.len(), 1);
    assert_eq!(graph.files["a.ts"].functions[0].name, "edit");
    let graph = compare(
        snapshot(&[("a.ts", old)]),
        snapshot(&[("a.ts", &format!("// comment\n{old}"))]),
    );
    assert_eq!(graph.files["a.ts"].change, Change::Modified);
    assert!(graph.files["a.ts"].functions.is_empty());
}
#[test]
fn named_methods_nested_functions_and_arrow_fields_are_qualified() {
    let graph=compare(Snapshot::default(),snapshot(&[("a.ts","class Worker { run() {} send = () => {}; } function outer() { function inner() {} } const object = { save() {}, load: () => {} }; const arrow = () => 1;")]));
    let names = graph.files["a.ts"]
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "Worker.run",
            "Worker.send",
            "outer",
            "outer.inner",
            "object.save",
            "object.load",
            "arrow"
        ])
    );
}
#[test]
fn overload_and_arrow_signature_changes_are_annotated() {
    let graph=compare(snapshot(&[("a.ts","function send(x: string): string; function send(x: any): any { return x; } const f: () => number = () => 1;")]),
        snapshot(&[("a.ts","function send(x: string | number): string; function send(x: any): any { return x; } const f: () => string = () => '1';")]));
    assert_eq!(graph.files["a.ts"].functions.len(), 2);
    assert!(graph.files["a.ts"]
        .functions
        .iter()
        .all(|f| f.change == Change::Modified));
}
#[test]
fn removed_functions_use_the_old_line_and_new_functions_use_the_new_line() {
    let graph = compare(
        snapshot(&[("a.ts", "\nfunction removed() {}\nfunction stable() {}")]),
        snapshot(&[("a.ts", "function stable() {}\n\n\nfunction added() {}")]),
    );
    let functions = &graph.files["a.ts"].functions;
    assert!(functions
        .iter()
        .any(|f| f.name == "removed" && f.line == 2 && f.change == Change::Removed));
    assert!(functions
        .iter()
        .any(|f| f.name == "added" && f.line == 4 && f.change == Change::Added));
}
#[test]
fn toggle_all_and_local_exceptions_survive_refresh() {
    let mut state = RadarState {
        graph: Some(Arc::new(chain())),
        ..RadarState::default()
    };
    state.relayout();
    let key = state.diagram.as_ref().unwrap().context[0].key.clone();
    state.toggle_all();
    assert!(state.diagram.as_ref().unwrap().all_expanded);
    state.toggle_chain(key.clone());
    assert!(!state.diagram.as_ref().unwrap().all_expanded);
    state.relayout();
    assert!(!state.diagram.as_ref().unwrap().all_expanded);
    state.toggle_chain(key);
    assert!(state.diagram.as_ref().unwrap().all_expanded);
    state.toggle_all();
    assert!(!state.diagram.as_ref().unwrap().all_expanded);
}
#[test]
fn stale_background_results_cannot_replace_a_newer_review() {
    let mut state = RadarState::default();
    let (sender, events) = mpsc::channel();
    state.events = events;
    state.generation = 2;
    state.loading = true;
    sender
        .send((1, Err("old comparison failed".into())))
        .unwrap();
    assert!(!state.poll());
    assert!(state.loading && state.error.is_none());
    sender.send((2, Ok(Graph::default()))).unwrap();
    assert!(state.poll());
    assert!(!state.loading && state.diagram.is_some());
}
fn commit(repo: &Repository, root: &Path, source: &str) -> git2::Oid {
    std::fs::write(root.join("main.ts"), source).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("main.ts")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Radar test", "radar@example.test").unwrap();
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents = parent.iter().collect::<Vec<_>>();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "test",
        &tree,
        &parents,
    )
    .unwrap()
}
#[test]
fn snapshots_respect_review_references_and_worktree_instead_of_index() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repo = Repository::init(root).unwrap();
    let first = commit(&repo, root, "function run() { return 1; }");
    let second = commit(&repo, root, "function run() { return 2; }");
    let from = first.to_string();
    let to = second.to_string();
    for reference in [
        CommitReference::Single(to.clone()),
        CommitReference::Range {
            from: from.clone(),
            to: to.clone(),
        },
        CommitReference::TripleDots {
            from: from.clone(),
            to: to.clone(),
        },
    ] {
        let graph = load(root, Some(&reference)).unwrap();
        assert_eq!(graph.files["main.ts"].functions[0].change, Change::Modified);
    }
    std::fs::write(root.join("main.ts"), "function run() { return 1; }").unwrap();
    assert_eq!(
        load(root, None).unwrap().files["main.ts"].change,
        Change::Modified
    );
    assert_eq!(
        load(root, Some(&CommitReference::RangeToWorkingTree { from }))
            .unwrap()
            .files["main.ts"]
            .change,
        Change::Unchanged
    );
    std::fs::write(root.join("main.ts"), "function broken(").unwrap();
    assert_eq!(
        load(root, Some(&CommitReference::Single(to)))
            .unwrap()
            .files["main.ts"]
            .functions
            .len(),
        1
    );
}
#[test]
fn unborn_repositories_include_untracked_sources_and_package_metadata() {
    let temp = tempfile::tempdir().unwrap();
    Repository::init(temp.path()).unwrap();
    std::fs::write(temp.path().join("main.ts"), "function start() {}").unwrap();
    std::fs::write(
        temp.path().join("package.json"),
        r#"{"name":"example","source":"./main.ts"}"#,
    )
    .unwrap();
    let graph = load(temp.path(), None).unwrap();
    assert_eq!(graph.files["main.ts"].change, Change::Added);
    assert_eq!(Diagram::new(&graph).groups.len(), 1);
}

#[test]
fn review_capture_includes_unchanged_config_and_unsupported_changed_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repo = Repository::init(root).unwrap();
    commit(&repo, root, "function run() { return 1; }");
    std::fs::write(root.join("base.json"), r#"{"compilerOptions":{"strict":true}}"#).unwrap();
    std::fs::write(root.join("config.jsonc"), "{ /* mapped runtime config */ \"enabled\": true }").unwrap();
    std::fs::write(root.join("other.rs"), "fn old() {}\n").unwrap();
    let (before, after, skipped, _, hashes) = review_sources(root, None, &["other.rs".into()]).unwrap();
    assert!(before.contains_key("main.ts"));
    assert!(after.contains_key("base.json"));
    assert!(after.contains_key("config.jsonc"));
    assert_eq!(after["other.rs"], "fn old() {}\n");
    assert!(!skipped.contains(&"other.rs".into()));
    std::fs::write(root.join("other.rs"), "fn new() {}\n").unwrap();
    let (_, _, _, _, updated) = review_sources(root, None, &["other.rs".into()]).unwrap();
    assert_ne!(hashes["other.rs"], updated["other.rs"]);
    // The extra policy capture does not add unrelated files to Radar.
    assert!(!load(root, None).unwrap().files.contains_key("other.rs"));
}

#[test]
fn review_capture_hashes_binary_changes_without_decoding_them_as_code() {
    let temp = tempfile::tempdir().unwrap();
    Repository::init(temp.path()).unwrap();
    std::fs::write(temp.path().join("image.bin"), [0xff, 1]).unwrap();
    let (_, after, skipped, _, hashes) = review_sources(temp.path(), None, &["image.bin".into()]).unwrap();
    assert!(!after.contains_key("image.bin"));
    assert!(skipped.contains(&"image.bin".into()));
    std::fs::write(temp.path().join("image.bin"), [0xff, 2]).unwrap();
    let (_, _, _, _, next) = review_sources(temp.path(), None, &["image.bin".into()]).unwrap();
    assert_ne!(hashes["image.bin"], next["image.bin"]);
}
#[test]
fn deep_chains_have_no_depth_limit_and_no_recursive_traversal() {
    let mut graph = Graph::default();
    for i in 0..2000 {
        let path = format!("{i:04}.ts");
        graph.files.insert(
            path.clone(),
            FileNode {
                path: path.clone(),
                package: "repo".into(),
                change: if i == 1999 {
                    Change::Modified
                } else {
                    Change::Unchanged
                },
                functions: vec![],
                entry: false,
            },
        );
        if i > 0 {
            graph.edges.push(Edge {
                from: format!("{:04}.ts", i - 1),
                to: path,
                change: Change::Unchanged,
            });
        }
    }
    let compact = Diagram::new(&graph);
    assert_eq!(compact.groups.len(), 2);
    assert_eq!(compact.context[0].files.len(), 1998);
    let expanded = Diagram::with_options(&graph, true, &BTreeSet::new(), &BTreeSet::new());
    assert_eq!(expanded.groups.len(), 2000);
    assert_eq!(expanded.connections.len(), 1999);
}
#[test]
fn long_edges_and_uneven_parents_do_not_intersect_file_cards() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[
            ("a.ts", "import './b'; import './d';"),
            (
                "b.ts",
                "import './c'; function b() {} function b2() {} function b3() {}",
            ),
            ("c.ts", "import './d';"),
            ("d.ts", "export const d=1;"),
        ]),
    );
    let diagram = Diagram::new(&graph);
    for edge in &diagram.connections {
        for pair in edge.points.windows(2) {
            for card in &diagram.groups {
                let (a, b) = (pair[0], pair[1]);
                let left = card.position.x;
                let right = left + card.width;
                let top = card.position.y;
                let bottom = top + card.height;
                let crosses = if a.x == b.x {
                    a.x > left && a.x < right && a.y.min(b.y) < bottom && a.y.max(b.y) > top
                } else {
                    a.y > top && a.y < bottom && a.x.min(b.x) < right && a.x.max(b.x) > left
                };
                assert!(!crosses, "edge crosses {}", card.path);
            }
        }
    }
}

#[test]
fn default_arrows_and_nested_objects_have_distinct_callable_annotations() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[(
            "main.ts",
            "export default () => 1; const o = { first: { run() {} }, second: { run() {} } };",
        )]),
    );
    let names = graph.files["main.ts"]
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from(["default", "o.first.run", "o.second.run"])
    );
}

#[test]
fn method_overloads_only_change_the_matching_qualified_method() {
    let a="class A { run(x: string): string; run(x: any): any { return x; } } class B { run(x: any): any { return x; } }";
    let b="class A { run(x: string | number): string; run(x: any): any { return x; } } class B { run(x: any): any { return x; } }";
    let graph = compare(snapshot(&[("main.ts", a)]), snapshot(&[("main.ts", b)]));
    assert_eq!(graph.files["main.ts"].functions.len(), 1);
    assert_eq!(graph.files["main.ts"].functions[0].name, "A.run");
}

#[test]
fn forest_order_uses_entry_points_and_roots_instead_of_descendant_names() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[
            ("package.json", r#"{"name":"demo","source":"main.ts"}"#),
            ("main.ts", "import './z';"),
            ("z.ts", "export const z = 1;"),
            ("worker.ts", "import './a';"),
            ("a.ts", "export const a = 1;"),
            ("other.ts", "import './b';"),
            ("b.ts", "export const b = 1;"),
        ]),
    );
    let diagram = Diagram::new(&graph);
    // Wider cards may wrap trees; preserve root order in reading order.
    let reading_order = |path| {
        let p = group(&diagram, path).position;
        (p.y, p.x)
    };
    assert!(reading_order("main.ts") < reading_order("other.ts"));
    assert!(reading_order("other.ts") < reading_order("worker.ts"));
}

fn assert_clear_edges(diagram: &Diagram) {
    for edge in &diagram.connections {
        for pair in edge.points.windows(2) {
            for card in &diagram.groups {
                let (a, b) = (pair[0], pair[1]);
                let (left, top) = (card.position.x, card.position.y);
                let (right, bottom) = (left + card.width, top + card.height);
                let crosses = if a.x == b.x {
                    a.x > left && a.x < right && a.y.min(b.y) < bottom && a.y.max(b.y) > top
                } else {
                    a.y > top && a.y < bottom && a.x.min(b.x) < right && a.x.max(b.x) > left
                };
                assert!(!crosses, "edge {a:?} -> {b:?} crosses {}", card.path);
            }
        }
    }
}

#[test]
fn large_function_cards_are_compact_and_expand_independently() {
    let old = (0..30)
        .map(|i| format!("function f{i}() {{ return 0; }}\n"))
        .collect::<String>();
    let new = (0..32)
        .filter(|i| *i != 0)
        .map(|i| format!("function f{i}() {{ return 1; }}\n"))
        .collect::<String>();
    let graph = compare(
        snapshot(&[("root.ts", "import './large';"), ("large.ts", &old)]),
        snapshot(&[("root.ts", "import './large';"), ("large.ts", &new)]),
    );
    let mut state = RadarState {
        graph: Some(Arc::new(graph)),
        ..RadarState::default()
    };
    state.relayout();
    let compact = state.diagram.clone().unwrap();
    let card = group(&compact, "large.ts");
    assert_eq!(card.counts, [2, 29, 1, 0]);
    assert!(card.height <= 70. && card.functions.is_empty());
    state.toggle_file("large.ts".into());
    let expanded = state.diagram.clone().unwrap();
    assert_eq!(group(&expanded, "large.ts").functions.len(), 32);
    assert_eq!(
        group(&expanded, "root.ts").height,
        group(&compact, "root.ts").height
    );
    assert_clear_edges(&expanded);
    state.relayout();
    assert!(group(state.diagram.as_ref().unwrap(), "large.ts").expanded);
    state.toggle_file("large.ts".into());
    assert_eq!(
        group(state.diagram.as_ref().unwrap(), "large.ts").height,
        card.height
    );
}

#[test]
fn many_test_roots_wrap_but_dependencies_stay_below_all_parents_with_and_without_filter() {
    let mut sources = (0..15)
        .map(|i| {
            (
                format!("tests/root{i:02}.test.ts"),
                "import { run } from '../app'; function testRun() { run(); }".to_string(),
            )
        })
        .collect::<Vec<_>>();
    sources.extend([
        (
            "main.ts".into(),
            "import { run } from './app'; function main() { run(); }".into(),
        ),
        (
            "app.ts".into(),
            "import { leaf } from './leaf'; export function run() { return leaf(); }".into(),
        ),
        (
            "leaf.ts".into(),
            "export function leaf() { return 1; }".into(),
        ),
    ]);
    let refs = sources
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect::<Vec<_>>();
    let mut graph = compare(Snapshot::default(), snapshot(&refs));
    for filtered in [false, true] {
        if filtered {
            filter_graph(&mut graph, &["*.test.ts".into()]);
        }
        for expanded in [BTreeSet::new(), graph.files.keys().cloned().collect()] {
            let diagram =
                Diagram::with_files(&graph, true, &BTreeSet::new(), &BTreeSet::new(), &expanded);
            assert!(diagram.width <= 1020., "width {}", diagram.width);
            for edge in &graph.edges {
                let (parent, child) = (group(&diagram, &edge.from), group(&diagram, &edge.to));
                assert!(child.position.y > parent.position.y + parent.height);
            }
            assert_clear_edges(&diagram);
            if filtered {
                assert_eq!(diagram.groups.len(), 3);
            } else {
                assert!(
                    diagram
                        .groups
                        .iter()
                        .filter(|g| g.root)
                        .map(|g| g.position.y as usize)
                        .collect::<BTreeSet<_>>()
                        .len()
                        > 1
                );
            }
        }
    }
}

#[test]
fn independent_forests_wrap_without_card_or_edge_overlap() {
    let mut sources = Vec::new();
    for i in 0..12 {
        sources.push((format!("root{i}.ts"), format!("import './leaf{i}';")));
        sources.push((
            format!("leaf{i}.ts"),
            "export function leaf() {}".to_string(),
        ));
    }
    let refs = sources
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect::<Vec<_>>();
    let diagram = Diagram::new(&compare(Snapshot::default(), snapshot(&refs)));
    assert!(diagram.width <= 1020.);
    assert!(diagram.groups.iter().any(|g| g.root && g.position.y > 20.));
    assert_clear_edges(&diagram);
}

#[test]
fn transitive_calls_include_unchanged_callers_and_callees_with_aliases_and_reexports() {
    let old = [
        ("main.ts", "import { run as start } from './app'; function main() { return start(); }"),
        ("app.ts", "import { helper as work } from './barrel'; export function run() { return work() + 1; }"),
        ("barrel.ts", "export { helper } from './helper';"),
        ("helper.ts", "import * as utils from './leaf'; export function helper() { return utils.leaf(); }"),
        ("leaf.ts", "export function leaf() { return 1; } export function unrelated() {}"),
        ("unrelated.ts", "import { leaf } from './leaf'; function unused() { return leaf(); }"),
    ];
    let mut new = old;
    new[1].1 =
        "import { helper as work } from './barrel'; export function run() { return work() + 2; }";
    let graph = compare(snapshot(&old), snapshot(&new));
    assert_eq!(graph.calls.len(), 3, "{:?}", graph.calls);
    for (path, name) in [
        ("main.ts", "main"),
        ("helper.ts", "helper"),
        ("leaf.ts", "leaf"),
    ] {
        let f = &graph.files[path].functions;
        assert_eq!(f.len(), 1, "{path}");
        assert_eq!(f[0].name, name);
        assert_eq!(f[0].change, Change::Unchanged);
    }
    assert!(graph.files["unrelated.ts"].functions.is_empty());
    let diagram = Diagram::with_options(&graph, true, &BTreeSet::new(), &BTreeSet::new());
    assert_eq!(diagram.groups.len(), 5);
    assert!(group(&diagram, "leaf.ts").position.y > group(&diagram, "helper.ts").position.y);
    assert!(group(&diagram, "helper.ts").position.y > group(&diagram, "barrel.ts").position.y);
}

#[test]
fn expanded_local_functions_follow_call_depth_and_preserve_recursive_cycles() {
    let graph = compare(Snapshot::default(), snapshot(&[("main.ts",
        "function leaf() { return 1; } function middle() { return leaf(); } function root() { return middle(); } function recursive() { return recursive(); }")]));
    let diagram = Diagram::with_files(
        &graph,
        false,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::from(["main.ts".into()]),
    );
    let rows = &group(&diagram, "main.ts").functions;
    let row = |name| rows.iter().find(|f| f.node.name == name).unwrap();
    assert!(row("root").position.y < row("middle").position.y);
    assert!(row("middle").position.y < row("leaf").position.y);
    assert_eq!(row("leaf").depth, 2);
    assert_eq!(row("recursive").depth, 0);
    assert!(row("root").calls[0].contains("middle()"));
}

#[test]
fn calls_resolve_default_imports_nested_functions_and_this_methods() {
    let graph = compare(Snapshot::default(), snapshot(&[
        ("main.ts", "import run from './default'; function start() { function inner() { return run(); } return inner(); } class Worker { run() { return this.send(); } send() {} }"),
        ("default.ts", "export default () => 1;"),
    ]));
    let pairs = graph
        .calls
        .iter()
        .map(|e| (e.from.1.as_str(), e.to.1.as_str()))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        pairs,
        BTreeSet::from([
            ("start", "start.inner"),
            ("start.inner", "default"),
            ("Worker.run", "Worker.send")
        ])
    );
}

#[test]
fn calls_do_not_guess_dynamic_shadowed_or_ambiguous_targets() {
    let graph = compare(Snapshot::default(), snapshot(&[
        ("main.ts", "import { run } from './barrel'; function a(run: () => void) { run(); } function b() { const run = other; run(); } function c() { object[method](); } function d() { run(); } function outer(run: () => void) { function inner() { run(); } }"),
        ("barrel.ts", "export * from './a'; export * from './b';"),
        ("a.ts", "export function run() {}"),
        ("b.ts", "export function run() {}"),
    ]));
    assert!(graph.calls.is_empty(), "{:?}", graph.calls);
}

#[test]
fn filtering_a_function_bridge_removes_detached_transitive_context() {
    let old = [
        (
            "app.ts",
            "import { run } from './bridge.test'; function start() { return run() + 1; }",
        ),
        (
            "bridge.test.ts",
            "import { leaf } from './leaf'; export function run() { return leaf(); }",
        ),
        ("leaf.ts", "export function leaf() { return 1; }"),
    ];
    let mut new = old;
    new[0].1 = "import { run } from './bridge.test'; function start() { return run() + 2; }";
    let mut graph = compare(snapshot(&old), snapshot(&new));
    assert!(!graph.files["leaf.ts"].functions.is_empty());
    filter_graph(&mut graph, &["*.test.ts".into()]);
    assert!(graph.calls.is_empty());
    assert!(graph.files["leaf.ts"].functions.is_empty());
}

#[test]
#[ignore = "set LUMINATTI_RADAR_REPO to inspect a local working-tree comparison"]
fn inspect_local_radar_layout() {
    let root = std::env::var("LUMINATTI_RADAR_REPO").expect("LUMINATTI_RADAR_REPO");
    let mut graph = load(Path::new(&root), None).unwrap();
    for filtered in [false, true] {
        if filtered {
            filter_graph(&mut graph, &["*.test.ts".into()]);
        }
        let diagram = Diagram::new(&graph);
        eprintln!("filtered={filtered}: {} changed files, {} cards, {} calls, {} roots, {} context chains, {} cycles, {:.0}x{:.0}, max card height {:.0}",
            diagram.changed_files, diagram.groups.len(), graph.calls.len(), diagram.groups.iter().filter(|g| g.root).count(),
            diagram.context.len(), diagram.cycles.len(), diagram.width, diagram.height,
            diagram.groups.iter().map(|g| g.height).fold(0_f32, f32::max));
        assert!(diagram.groups.iter().all(|g| g.height <= 70.));
        assert_clear_edges(&diagram);
    }
}

#[test]
fn import_retargeting_keeps_both_call_targets_without_marking_the_caller_modified() {
    let old = [
        (
            "app.ts",
            "import { run } from './a'; function start() { return run(); }",
        ),
        ("a.ts", "export function run() { return 1; }"),
        ("b.ts", "export function run() { return 2; }"),
    ];
    let mut new = old;
    new[0].1 = "import { run } from './b'; function start() { return run(); }";
    let graph = compare(snapshot(&old), snapshot(&new));
    assert_eq!(graph.files["app.ts"].functions[0].change, Change::Unchanged);
    assert_eq!(graph.calls.len(), 2);
    assert!(graph
        .calls
        .iter()
        .any(|e| e.to.0 == "a.ts" && e.change == Change::Removed));
    assert!(graph
        .calls
        .iter()
        .any(|e| e.to.0 == "b.ts" && e.change == Change::Added));
    assert_eq!(Diagram::new(&graph).groups.len(), 3);
}

#[test]
fn wider_labels_and_expanded_functions_stay_inside_cards() {
    let graph = compare(
        Snapshot::default(),
        snapshot(&[
            ("main.ts", "import './very-long-dependency-module-name';"),
            (
                "very-long-dependency-module-name.ts",
                "export function aVeryLongFunctionNameWithUsefulContext() {}",
            ),
        ]),
    );
    let compact = Diagram::new(&graph);
    assert!(
        group(&compact, "very-long-dependency-module-name.ts").width
            > group(&compact, "main.ts").width
    );
    let expanded = Diagram::with_files(
        &graph,
        true,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &graph.files.keys().cloned().collect(),
    );
    for card in &expanded.groups {
        for function in &card.functions {
            assert!(function.position.x + function.width <= card.position.x + card.width);
            assert!(function.position.y + FUNCTION_ROW_HEIGHT <= card.position.y + card.height);
        }
    }
    assert_clear_edges(&expanded);
}

#[test]
fn stale_tala_results_cannot_undo_expansion_or_refresh() {
    let mut state = RadarState {
        graph: Some(Arc::new(chain())),
        ..RadarState::default()
    };
    state.relayout();
    let mut old = state.diagram.as_ref().unwrap().as_ref().clone();
    old.tala = true;
    let generation = state.layout_generation;
    let (sender, events) = mpsc::channel();
    state.layout_events = events;
    state.toggle_all();
    sender.send((generation, old.clone())).unwrap();
    assert!(!state.poll());
    assert!(state.diagram.as_ref().unwrap().all_expanded);
    let generation = state.layout_generation;
    state.refresh(PathBuf::from("unused"), None, vec![]);
    sender.send((generation, old)).unwrap();
    assert!(!state.poll());
    assert!(state.diagram.is_none());
}

#[test]
#[ignore = "build helper and set absolute LUMINATTI_RADAR_LAYOUT to exercise TALA"]
fn tala_routes_real_graphs_without_hiding_cards_or_crossing_them() {
    assert!(
        tala::executable().is_some(),
        "build scripts/build-radar-layout.sh first"
    );
    let fixtures = [
        chain(),
        compare(
            Snapshot::default(),
            snapshot(&[
                ("a.ts", "import './b'; import './d';"),
                ("b.ts", "import './c'; function b() {} function other() {}"),
                ("c.ts", "import './d';"),
                ("d.ts", "export function dependency() {}"),
                ("independent.ts", "import './leaf';"),
                ("leaf.ts", "export const value=1;"),
            ]),
        ),
        compare(
            Snapshot::default(),
            snapshot(&[
                ("a.ts", "import './b';"),
                ("b.ts", "import './a'; import './c';"),
                ("c.ts", "export function dependency() {}"),
            ]),
        ),
    ];
    for graph in fixtures {
        for expanded in [false, true] {
            let files = if expanded {
                graph.files.keys().cloned().collect()
            } else {
                BTreeSet::new()
            };
            let cycles = Diagram::new(&graph)
                .cycles
                .iter()
                .map(|c| c.key.clone())
                .collect();
            let native = Diagram::with_files(&graph, expanded, &BTreeSet::new(), &cycles, &files);
            let diagram = Diagram::with_tala(&graph, expanded, &BTreeSet::new(), &cycles, &files);
            assert!(diagram.tala, "TALA unexpectedly fell back");
            assert_eq!(diagram.groups.len(), native.groups.len());
            assert_eq!(diagram.context.len(), native.context.len());
            assert_eq!(diagram.connections.len(), native.connections.len());
            assert_clear_edges(&diagram);
            for card in &diagram.groups {
                assert!(card.position.x >= 0. && card.position.y >= 0.);
                assert!(card.position.x + card.width <= diagram.width);
                assert!(card.position.y + card.height <= diagram.height);
            }
        }
    }
}
