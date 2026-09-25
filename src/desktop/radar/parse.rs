//! File imports and named callable changes. File edges do not imply call edges.
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Node, Parser};

#[derive(Clone, Debug, Default)]
pub(super) struct Module {
    pub functions: BTreeMap<String, Function>,
    pub dependencies: BTreeSet<String>,
    pub fingerprint: String,
    pub unresolved_dynamic: usize,
    pub parse_error: bool,
    pub imports: BTreeMap<String, (String, String)>,
    pub exports: BTreeMap<String, (Option<String>, String)>,
    pub export_stars: Vec<String>,
    overloads: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub(super) struct Function {
    pub line: usize,
    pub signature: String,
    pub body: String,
    pub calls: BTreeSet<String>,
    pub shadows: BTreeSet<String>,
}

fn binding_names(node: Node<'_>, source: &str, names: &mut BTreeSet<String>) {
    match node.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            names.insert(text(node, source).into());
        }
        "pair_pattern" => {
            if let Some(value) = node.child_by_field_name("value") {
                binding_names(value, source, names);
            }
        }
        "required_parameter" | "optional_parameter" | "assignment_pattern" => {
            if let Some(pattern) = node
                .child_by_field_name("pattern")
                .or_else(|| node.child_by_field_name("left"))
            {
                binding_names(pattern, source, names);
            }
        }
        "object_pattern" | "array_pattern" | "formal_parameters" | "rest_pattern" => {
            for child in children(node) {
                binding_names(child, source, names);
            }
        }
        _ => {}
    }
}

fn call_info(
    function: Node<'_>,
    body: Node<'_>,
    source: &str,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut calls = BTreeSet::new();
    let mut shadows = BTreeSet::new();
    if let Some(parameters) = function
        .child_by_field_name("parameters")
        .or_else(|| function.child_by_field_name("parameter"))
    {
        binding_names(parameters, source, &mut shadows);
    }
    let mut pending = vec![body];
    while let Some(node) = pending.pop() {
        if callable(node) {
            continue;
        }
        if matches!(
            node.kind(),
            "variable_declarator" | "catch_clause" | "class_declaration"
        ) {
            if !node.child_by_field_name("value").is_some_and(callable) {
                if let Some(name) = node
                    .child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("parameter"))
                {
                    binding_names(name, source, &mut shadows);
                }
            }
        }
        if node.kind() == "call_expression" {
            if let Some(target) = node.child_by_field_name("function") {
                // Only static identifiers/member chains, never computed or
                // dynamically returned callables.
                fn target_name(node: Node<'_>, source: &str) -> Option<String> {
                    match node.kind() {
                        "identifier" | "this" | "property_identifier" => {
                            Some(text(node, source).into())
                        }
                        "member_expression" => Some(format!(
                            "{}.{}",
                            target_name(node.child_by_field_name("object")?, source)?,
                            target_name(node.child_by_field_name("property")?, source)?
                        )),
                        _ => None,
                    }
                }
                if let Some(name) = target_name(target, source) {
                    calls.insert(name);
                }
            }
        }
        pending.extend(children(node));
    }
    (calls, shadows)
}

fn module_bindings(root: Node<'_>, source: &str, module: &mut Module) {
    for node in children(root) {
        let path = node
            .child_by_field_name("source")
            .map(|p| text(p, source).trim_matches(['\'', '"']).to_string());
        if node.kind() == "import_statement" && !text(node, source).starts_with("import type ") {
            if let Some(path) = path {
                for clause in children(node)
                    .into_iter()
                    .filter(|n| n.kind() == "import_clause")
                {
                    for item in children(clause) {
                        match item.kind() {
                            "identifier" => {
                                module.imports.insert(
                                    text(item, source).into(),
                                    (path.clone(), "default".into()),
                                );
                            }
                            "namespace_import" => {
                                if let Some(name) = item.named_child(0) {
                                    module.imports.insert(
                                        text(name, source).into(),
                                        (path.clone(), "*".into()),
                                    );
                                }
                            }
                            "named_imports" => {
                                for spec in children(item) {
                                    if text(spec, source).starts_with("type ") {
                                        continue;
                                    }
                                    if let Some(name) = spec.child_by_field_name("name") {
                                        let alias =
                                            spec.child_by_field_name("alias").unwrap_or(name);
                                        module.imports.insert(
                                            text(alias, source).into(),
                                            (path.clone(), text(name, source).into()),
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        } else if node.kind() == "export_statement"
            && !text(node, source).starts_with("export type ")
        {
            if let Some(clause) = children(node)
                .into_iter()
                .find(|n| n.kind() == "export_clause")
            {
                for spec in children(clause) {
                    if text(spec, source).starts_with("type ") {
                        continue;
                    }
                    if let Some(name) = spec.child_by_field_name("name") {
                        let alias = spec.child_by_field_name("alias").unwrap_or(name);
                        module.exports.insert(
                            text(alias, source).into(),
                            (path.clone(), text(name, source).into()),
                        );
                    }
                }
            } else if let Some(path) = path {
                // Namespace re-exports need a namespace binding, not an
                // unqualified export-star lookup.
                if !children(node)
                    .iter()
                    .any(|n| n.kind() == "namespace_export")
                {
                    module.export_stars.push(path);
                }
            } else {
                let declaration = node
                    .child_by_field_name("declaration")
                    .or_else(|| node.child_by_field_name("value"));
                if let Some(declaration) = declaration {
                    let default = text(node, source).starts_with("export default ");
                    if let Some(name) = declaration.child_by_field_name("name") {
                        module.exports.insert(
                            if default {
                                "default".into()
                            } else {
                                text(name, source).into()
                            },
                            (None, text(name, source).into()),
                        );
                    } else if default {
                        let name = if declaration.kind() == "identifier" {
                            text(declaration, source)
                        } else {
                            "default"
                        };
                        module.exports.insert("default".into(), (None, name.into()));
                    } else {
                        for binding in children(declaration) {
                            if let Some(name) = binding.child_by_field_name("name") {
                                module.exports.insert(
                                    text(name, source).into(),
                                    (None, text(name, source).into()),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or_default()
}

fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

// Ignore formatting/comments in callable annotations, but preserve literal text.
fn fingerprint(node: Node<'_>, source: &str, skip: Option<usize>) -> String {
    fn visit(node: Node<'_>, source: &str, skip: Option<usize>, hash: &mut Sha256) {
        if node.kind() == "comment" || Some(node.id()) == skip {
            return;
        }
        if node.child_count() == 0 {
            hash.update(node.kind().as_bytes());
            hash.update([0]);
            hash.update(text(node, source).as_bytes());
            hash.update([0]);
        } else {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                visit(child, source, skip, hash);
            }
        }
    }
    let mut hash = Sha256::new();
    visit(node, source, skip, &mut hash);
    format!("{:x}", hash.finalize())
}

fn callable(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
    )
}

fn collect(node: Node<'_>, source: &str, scope: &str, module: &mut Module) {
    if matches!(node.kind(), "import_statement" | "export_statement") {
        if let Some(path) = node.child_by_field_name("source") {
            module
                .dependencies
                .insert(text(path, source).trim_matches(['\'', '"']).into());
        }
    }
    if node.kind() == "call_expression"
        && node
            .child_by_field_name("function")
            .is_some_and(|n| n.kind() == "import")
    {
        let arg = node
            .child_by_field_name("arguments")
            .and_then(|args| args.named_child(0));
        if let Some(arg) = arg.filter(|arg| arg.kind() == "string") {
            module
                .dependencies
                .insert(text(arg, source).trim_matches(['\'', '"']).into());
        } else {
            module.unresolved_dynamic += 1;
        }
    }
    // CommonJS and TS import-equals need scope-aware resolution. Report them,
    // rather than guessing a target from a possibly shadowed require binding.
    if node.kind() == "import_require_clause"
        || (node.kind() == "call_expression"
            && node
                .child_by_field_name("function")
                .is_some_and(|n| text(n, source) == "require"))
    {
        module.unresolved_dynamic += 1;
    }

    if matches!(node.kind(), "function_signature" | "method_signature") {
        if let Some(name) = node.child_by_field_name("name") {
            let name = if scope.is_empty() {
                text(name, source).into()
            } else {
                format!("{scope}.{}", text(name, source))
            };
            module
                .overloads
                .entry(name)
                .or_default()
                .push_str(&fingerprint(node, source, None));
        }
    }
    let value = node.child_by_field_name("value");
    let function = if callable(node) {
        Some(node)
    } else {
        value.filter(|value| callable(*value))
    };
    let mut next_scope = scope.to_string();
    if let Some(function) = function {
        // The variable/property owns a callable expression's name; don't emit it twice.
        let expression_owned = callable(node)
            && node.parent().is_some_and(|parent| {
                parent
                    .child_by_field_name("value")
                    .is_some_and(|value| value.id() == node.id())
            });
        if !expression_owned {
            let name = node
                .child_by_field_name("name")
                .or_else(|| node.child_by_field_name("key"))
                .map(|name| text(name, source).to_string())
                .or_else(|| {
                    (node.kind() == "export_statement").then(|| {
                        function
                            .child_by_field_name("name")
                            .map(|name| text(name, source).to_string())
                            .unwrap_or_else(|| "default".into())
                    })
                })
                .or_else(|| {
                    node.parent()
                        .filter(|p| p.kind() == "export_statement")
                        .map(|_| "default".to_string())
                });
            if let Some(name) = name {
                next_scope = if scope.is_empty() {
                    name
                } else {
                    format!("{scope}.{name}")
                };
                if let Some(body) = function.child_by_field_name("body") {
                    let (calls, shadows) = call_info(function, body, source);
                    module.functions.insert(
                        next_scope.clone(),
                        Function {
                            line: node.start_position().row + 1,
                            signature: fingerprint(node, source, Some(body.id())),
                            body: fingerprint(body, source, None),
                            calls,
                            shadows,
                        },
                    );
                }
            }
        }
    } else if matches!(
        node.kind(),
        "class_declaration" | "class" | "object" | "internal_module"
    ) {
        if let Some(name) = node.child_by_field_name("name").or_else(|| {
            node.parent()
                .filter(|p| {
                    matches!(
                        p.kind(),
                        "variable_declarator" | "pair" | "public_field_definition"
                    )
                })
                .and_then(|p| {
                    p.child_by_field_name("name")
                        .or_else(|| p.child_by_field_name("key"))
                })
        }) {
            next_scope = if scope.is_empty() {
                text(name, source).into()
            } else {
                format!("{scope}.{}", text(name, source))
            };
        }
    }
    for child in children(node) {
        collect(child, source, &next_scope, module);
    }
}

pub(super) fn parse(path: &str, source: &str) -> Module {
    let mut module = Module {
        fingerprint: format!("{:x}", Sha256::digest(source.as_bytes())),
        ..Module::default()
    };
    let mut parser = Parser::new();
    let language = if path.ends_with(".tsx") {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    } else if matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str()),
        Some("js" | "jsx" | "mjs" | "cjs")
    ) {
        tree_sitter_javascript::LANGUAGE.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    if parser.set_language(&language).is_err() {
        module.parse_error = true;
        return module;
    }
    let Some(tree) = parser.parse(source, None) else {
        module.parse_error = true;
        return module;
    };
    if tree.root_node().has_error() {
        module.parse_error = true;
        return module;
    }
    collect(tree.root_node(), source, "", &mut module);
    module_bindings(tree.root_node(), source, &mut module);
    // Overloads use the same qualified scope as their implementation.
    for (name, signature) in std::mem::take(&mut module.overloads) {
        if let Some(function) = module.functions.get_mut(&name) {
            function.signature.push_str(&signature);
        }
    }
    module
}
