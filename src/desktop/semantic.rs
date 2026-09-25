//! Deterministic TypeScript/TSX structure and change analysis.
//!
//! Tree-sitter supplies facts that can be reproduced from the compared source.
//! LSP data is layered on later and never replaces these structural results.

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};
use tree_sitter::{Language, Node, Parser};

use crate::command::diff::highlight::{syntax_ranges_by_line, SyntaxRange};
use crate::command::diff::types::FileDiff;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Type,
    Enum,
    Variable,
}

impl SymbolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Type => "type",
            Self::Enum => "enum",
            Self::Variable => "function variable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub signature: String,
    body_hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticChangeKind {
    Added,
    Removed,
    Renamed,
    Moved,
    SignatureChanged,
    ImplementationChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticChange {
    pub kind: SemanticChangeKind,
    pub symbol_kind: SymbolKind,
    pub name: String,
    pub previous_name: Option<String>,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub detail: String,
}

#[derive(Clone, Debug, Default)]
pub struct FileSemanticAnalysis {
    pub symbols: Vec<FileSymbol>,
    pub changes: Vec<SemanticChange>,
    pub old_syntax: HashMap<usize, Vec<SyntaxRange>>,
    pub new_syntax: HashMap<usize, Vec<SyntaxRange>>,
    pub parse_errors: usize,
}

pub fn is_typescript_path(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "mts" | "cts")
    )
}

pub fn analyze_typescript(diff: &FileDiff) -> FileSemanticAnalysis {
    if !is_typescript_path(&diff.filename) || diff.is_binary {
        return FileSemanticAnalysis::default();
    }

    let old = parse_symbols(&diff.old_content, &diff.filename);
    let new = parse_symbols(&diff.new_content, &diff.filename);
    FileSemanticAnalysis {
        changes: compare_symbols(&old.symbols, &new.symbols),
        symbols: new.symbols,
        old_syntax: syntax_ranges_by_line(&diff.old_content, &diff.filename),
        new_syntax: syntax_ranges_by_line(&diff.new_content, &diff.filename),
        parse_errors: old.errors + new.errors,
    }
}

struct ParsedSymbols {
    symbols: Vec<FileSymbol>,
    errors: usize,
}

fn parse_symbols(source: &str, path: &str) -> ParsedSymbols {
    if source.is_empty() {
        return ParsedSymbols {
            symbols: Vec::new(),
            errors: 0,
        };
    }
    let language: Language = if path.ends_with(".tsx") {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return ParsedSymbols {
            symbols: Vec::new(),
            errors: 1,
        };
    }
    let Some(tree) = parser.parse(source, None) else {
        return ParsedSymbols {
            symbols: Vec::new(),
            errors: 1,
        };
    };

    let mut symbols = Vec::new();
    let mut errors = 0;
    collect_symbols(tree.root_node(), source, &mut symbols, &mut errors);
    ParsedSymbols { symbols, errors }
}

fn collect_symbols(
    node: Node<'_>,
    source: &str,
    symbols: &mut Vec<FileSymbol>,
    errors: &mut usize,
) {
    if node.is_error() || node.is_missing() {
        *errors += 1;
    }
    if let Some(symbol) = symbol_from_node(node, source) {
        symbols.push(symbol);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_symbols(child, source, symbols, errors);
    }
}

fn symbol_from_node(node: Node<'_>, source: &str) -> Option<FileSymbol> {
    let kind = match node.kind() {
        "function_declaration" | "generator_function_declaration" => SymbolKind::Function,
        "method_definition" | "method_signature" => SymbolKind::Method,
        "class_declaration" => SymbolKind::Class,
        "interface_declaration" => SymbolKind::Interface,
        "type_alias_declaration" => SymbolKind::Type,
        "enum_declaration" => SymbolKind::Enum,
        "variable_declarator" => {
            let value = node.child_by_field_name("value")?;
            if !matches!(value.kind(), "arrow_function" | "function_expression") {
                return None;
            }
            SymbolKind::Variable
        }
        _ => return None,
    };
    let name_node = node.child_by_field_name("name")?;
    let name = name_node.utf8_text(source.as_bytes()).ok()?.to_string();
    let text = node.utf8_text(source.as_bytes()).ok()?;
    let signature_end = node
        .child_by_field_name("body")
        .map(|body| body.start_byte().saturating_sub(node.start_byte()))
        .unwrap_or(text.len());
    let signature = normalize_whitespace(&text[..signature_end.min(text.len())]);
    let relative_name_start = name_node.start_byte().saturating_sub(node.start_byte());
    let relative_name_end = name_node.end_byte().saturating_sub(node.start_byte());
    let mut structural_body = text.to_string();
    if relative_name_end <= structural_body.len()
        && structural_body.is_char_boundary(relative_name_start)
        && structural_body.is_char_boundary(relative_name_end)
    {
        structural_body.replace_range(relative_name_start..relative_name_end, "<symbol>");
    }
    let body_hash = digest(&normalize_whitespace(&structural_body));
    Some(FileSymbol {
        name,
        kind,
        line: node.start_position().row + 1,
        column: name_node.start_position().column,
        end_line: node.end_position().row + 1,
        signature,
        body_hash,
    })
}

fn compare_symbols(old: &[FileSymbol], new: &[FileSymbol]) -> Vec<SemanticChange> {
    let mut changes = Vec::new();
    let mut matched_old = HashSet::new();
    let mut matched_new = HashSet::new();

    for (new_index, current) in new.iter().enumerate() {
        if let Some((old_index, previous)) = old.iter().enumerate().find(|(index, symbol)| {
            !matched_old.contains(index)
                && symbol.name == current.name
                && symbol.kind == current.kind
        }) {
            matched_old.insert(old_index);
            matched_new.insert(new_index);
            if previous.signature != current.signature {
                changes.push(change(
                    SemanticChangeKind::SignatureChanged,
                    previous,
                    current,
                    format!("{} → {}", previous.signature, current.signature),
                ));
            } else if previous.body_hash == current.body_hash
                && previous.line.abs_diff(current.line) > 2
            {
                changes.push(change(
                    SemanticChangeKind::Moved,
                    previous,
                    current,
                    format!("Line {} → {}", previous.line, current.line),
                ));
            } else if previous.body_hash != current.body_hash {
                changes.push(change(
                    SemanticChangeKind::ImplementationChanged,
                    previous,
                    current,
                    "Body changed while the signature stayed stable".to_string(),
                ));
            }
        }
    }

    // A stable body and kind with a changed identifier is strong evidence of a
    // rename. Avoid guessing when more than one candidate has the same body.
    for (new_index, current) in new.iter().enumerate() {
        if matched_new.contains(&new_index) {
            continue;
        }
        let candidates = old
            .iter()
            .enumerate()
            .filter(|(index, previous)| {
                !matched_old.contains(index)
                    && previous.kind == current.kind
                    && previous.body_hash == current.body_hash
            })
            .collect::<Vec<_>>();
        if let [(old_index, previous)] = candidates.as_slice() {
            matched_old.insert(*old_index);
            matched_new.insert(new_index);
            changes.push(SemanticChange {
                kind: SemanticChangeKind::Renamed,
                symbol_kind: current.kind,
                name: current.name.clone(),
                previous_name: Some(previous.name.clone()),
                old_line: Some(previous.line),
                new_line: Some(current.line),
                detail: format!("{} retained its structural body", current.kind.label()),
            });
        }
    }

    for (index, previous) in old.iter().enumerate() {
        if !matched_old.contains(&index) {
            changes.push(SemanticChange {
                kind: SemanticChangeKind::Removed,
                symbol_kind: previous.kind,
                name: previous.name.clone(),
                previous_name: None,
                old_line: Some(previous.line),
                new_line: None,
                detail: previous.signature.clone(),
            });
        }
    }
    for (index, current) in new.iter().enumerate() {
        if !matched_new.contains(&index) {
            changes.push(SemanticChange {
                kind: SemanticChangeKind::Added,
                symbol_kind: current.kind,
                name: current.name.clone(),
                previous_name: None,
                old_line: None,
                new_line: Some(current.line),
                detail: current.signature.clone(),
            });
        }
    }

    changes.sort_by_key(|item| std::cmp::Reverse(item.new_line.or(item.old_line).unwrap_or(0)));
    changes
}

fn change(
    kind: SemanticChangeKind,
    previous: &FileSymbol,
    current: &FileSymbol,
    detail: String,
) -> SemanticChange {
    SemanticChange {
        kind,
        symbol_kind: current.kind,
        name: current.name.clone(),
        previous_name: None,
        old_line: Some(previous.line),
        new_line: Some(current.line),
        detail,
    }
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn digest(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::diff::types::FileStatus;

    fn analyze(old: &str, new: &str) -> FileSemanticAnalysis {
        analyze_typescript(&FileDiff {
            filename: "src/example.ts".into(),
            old_content: old.into(),
            new_content: new.into(),
            status: FileStatus::Modified,
            is_binary: false,
        })
    }

    #[test]
    fn detects_signature_and_implementation_changes() {
        let result = analyze(
            "export function total(value: number) { return value; }",
            "export function total(value: string): number { return value.length; }",
        );
        assert_eq!(result.symbols[0].name, "total");
        assert!(result
            .changes
            .iter()
            .any(|change| change.kind == SemanticChangeKind::SignatureChanged));
        assert!(!result.new_syntax.is_empty());
    }

    #[test]
    fn detects_added_and_removed_symbols() {
        let result = analyze(
            "function before() { return 1; }",
            "function after() { return 2; }",
        );
        assert!(result
            .changes
            .iter()
            .any(|change| change.kind == SemanticChangeKind::Removed));
        assert!(result
            .changes
            .iter()
            .any(|change| change.kind == SemanticChangeKind::Added));
    }

    #[test]
    fn detects_a_structurally_stable_rename() {
        let result = analyze(
            "function before(value: number) { return value + 1; }",
            "function after(value: number) { return value + 1; }",
        );
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].kind, SemanticChangeKind::Renamed);
        assert_eq!(result.changes[0].previous_name.as_deref(), Some("before"));
        assert_eq!(result.changes[0].name, "after");
    }

    #[test]
    fn ignores_non_typescript_files() {
        let mut diff = FileDiff {
            filename: "src/example.rs".into(),
            old_content: "fn old() {}".into(),
            new_content: "fn new() {}".into(),
            status: FileStatus::Modified,
            is_binary: false,
        };
        let result = analyze_typescript(&diff);
        assert!(result.symbols.is_empty());
        diff.filename = "src/example.tsx".into();
        diff.old_content = "const oldValue = 1;".into();
        diff.new_content = "const newValue = 2;".into();
        assert_eq!(analyze_typescript(&diff).parse_errors, 0);
    }
}
