//! Pure layout and selection helpers for the desktop review workspace.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use super::{lsp::LspSymbol, model, NativeRow};

#[derive(Default)]
struct FileTree {
    directories: BTreeMap<String, FileTree>,
    files: Vec<usize>,
}

pub(super) enum FileTreeEntry {
    Directory {
        name: String,
        path: String,
        depth: usize,
        collapsed: bool,
    },
    File {
        index: usize,
        depth: usize,
    },
}

impl FileTreeEntry {
    pub(super) fn depth(&self) -> usize {
        match self {
            Self::Directory { depth, .. } | Self::File { depth, .. } => *depth,
        }
    }
}

/// A contiguous range of lines which has the same contents in both panels.
/// Keeping the file path in the key lets a file retain its collapsed sections
/// while the reviewer moves between tabs.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct UnchangedSection {
    pub(super) file_path: String,
    pub(super) start_row: usize,
    pub(super) end_row: usize,
    #[serde(default)]
    pub(super) label: Option<String>,
}

impl PartialEq for UnchangedSection {
    fn eq(&self, other: &Self) -> bool {
        self.file_path == other.file_path
            && self.start_row == other.start_row
            && self.end_row == other.end_row
    }
}

impl Eq for UnchangedSection {}

impl Hash for UnchangedSection {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.file_path.hash(state);
        self.start_row.hash(state);
        self.end_row.hash(state);
    }
}

#[derive(Clone)]
pub(super) enum DiffDisplayRow {
    Code { source_row: usize },
    CollapsedUnchanged { section: UnchangedSection },
}

/// Return the first row of each contiguous changed block in a file. A block
/// represents one navigable change even when it spans multiple modified lines.
pub(super) fn change_start_rows(rows: &[NativeRow]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut in_change = false;

    for (index, row) in rows.iter().enumerate() {
        let changed = !matches!(row.change, crate::command::diff::types::ChangeType::Equal);
        if changed && !in_change {
            starts.push(index);
        }
        in_change = changed;
    }

    starts
}

pub(super) fn unchanged_sections(file_path: &str, rows: &[NativeRow]) -> Vec<UnchangedSection> {
    let mut sections = Vec::new();
    let mut start = None;

    for (index, row) in rows.iter().enumerate() {
        if matches!(row.change, crate::command::diff::types::ChangeType::Equal) {
            start.get_or_insert(index);
        } else if let Some(start_row) = start.take() {
            sections.push(UnchangedSection {
                file_path: file_path.to_string(),
                start_row,
                end_row: index,
                label: None,
            });
        }
    }

    if let Some(start_row) = start {
        sections.push(UnchangedSection {
            file_path: file_path.to_string(),
            start_row,
            end_row: rows.len(),
            label: None,
        });
    }

    sections
}

/// Divide unchanged runs at declaration boundaries. LSP ranges take priority;
/// the parsed TypeScript symbols keep folding useful while the server starts.
pub(super) fn logical_unchanged_sections(
    file: &model::NativeFile,
    lsp_symbols: &[LspSymbol],
) -> Vec<UnchangedSection> {
    let mut symbols = if lsp_symbols.is_empty() {
        file.semantic
            .symbols
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.line, symbol.end_line))
            .collect::<Vec<_>>()
    } else {
        lsp_symbols
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.line, symbol.end_line))
            .collect::<Vec<_>>()
    };
    if symbols.is_empty() {
        return unchanged_sections(&file.path, &file.rows);
    }
    let line_rows = file
        .rows
        .iter()
        .enumerate()
        .filter_map(|(row, line)| line.new_number.map(|number| (number, row)))
        .collect::<HashMap<_, _>>();
    // Resolve declaration boundaries once. Scanning every symbol for every
    // unchanged run grows quadratically on files with many declarations.
    let mut boundary_rows = symbols
        .iter()
        .flat_map(|(_, start, end)| {
            [*start, *end]
                .into_iter()
                .filter_map(|line| line_rows.get(&line).copied())
        })
        .collect::<Vec<_>>();
    boundary_rows.sort_unstable();
    boundary_rows.dedup();
    symbols.sort_unstable_by_key(|(_, start, _)| *start);
    let mut next_symbol = 0;
    let mut active =
        std::collections::BinaryHeap::<std::cmp::Reverse<(usize, usize, usize)>>::new();
    let mut result = Vec::new();
    for run in unchanged_sections(&file.path, &file.rows) {
        let mut boundaries = vec![run.start_row, run.end_row];
        let first = boundary_rows.partition_point(|&row| row <= run.start_row);
        let last = boundary_rows.partition_point(|&row| row < run.end_row);
        boundaries.extend_from_slice(&boundary_rows[first..last]);
        boundaries.sort_unstable();
        for pair in boundaries.windows(2) {
            let start_row = pair[0];
            let end_row = pair[1];
            if end_row - start_row < 2 && !symbols.is_empty() {
                continue;
            }
            let line = file.rows[start_row].new_number.unwrap_or(0);
            while next_symbol < symbols.len() && symbols[next_symbol].1 <= line {
                let (_, start, end) = symbols[next_symbol];
                active.push(std::cmp::Reverse((
                    end.saturating_sub(start),
                    end,
                    next_symbol,
                )));
                next_symbol += 1;
            }
            while active
                .peek()
                .is_some_and(|std::cmp::Reverse((_, end, _))| *end < line)
            {
                active.pop();
            }
            let label = active
                .peek()
                .map(|std::cmp::Reverse((_, _, index))| symbols[*index].0.to_string());
            result.push(UnchangedSection {
                file_path: file.path.clone(),
                start_row,
                end_row,
                label,
            });
        }
    }
    result
}

pub(super) fn display_rows_for_file(
    file: &model::NativeFile,
    collapsed_sections: &HashSet<UnchangedSection>,
    lsp_symbols: &[LspSymbol],
) -> Vec<DiffDisplayRow> {
    let sections = logical_unchanged_sections(file, lsp_symbols);
    display_rows_from_sections(file.rows.len(), &sections, collapsed_sections)
}

pub(super) fn display_rows_from_sections(
    row_count: usize,
    sections: &[UnchangedSection],
    collapsed_sections: &HashSet<UnchangedSection>,
) -> Vec<DiffDisplayRow> {
    let mut display_rows = Vec::with_capacity(row_count);
    let mut source_row = 0;

    for section in sections {
        let end_row = section.end_row;
        while source_row < section.start_row {
            display_rows.push(DiffDisplayRow::Code { source_row });
            source_row += 1;
        }

        if collapsed_sections.contains(section) {
            display_rows.push(DiffDisplayRow::CollapsedUnchanged {
                section: section.clone(),
            });
        } else {
            while source_row < section.end_row {
                display_rows.push(DiffDisplayRow::Code { source_row });
                source_row += 1;
            }
        }

        source_row = end_row;
    }

    while source_row < row_count {
        display_rows.push(DiffDisplayRow::Code { source_row });
        source_row += 1;
    }

    display_rows
}

pub(super) fn build_file_tree_entries(
    paths: &[String],
    collapsed_directories: &HashSet<String>,
    compact_directories: bool,
) -> Vec<FileTreeEntry> {
    let mut root = FileTree::default();
    for (index, path) in paths.iter().enumerate() {
        let mut node = &mut root;
        let mut components = path.split('/').peekable();
        while let Some(component) = components.next() {
            if components.peek().is_some() {
                node = node.directories.entry(component.to_string()).or_default();
            } else {
                node.files.push(index);
            }
        }
    }

    fn append_entries(
        node: &FileTree,
        parent_path: &str,
        depth: usize,
        collapsed_directories: &HashSet<String>,
        compact_directories: bool,
        entries: &mut Vec<FileTreeEntry>,
    ) {
        for (name, child) in &node.directories {
            let mut path = if parent_path.is_empty() {
                name.clone()
            } else {
                format!("{parent_path}/{name}")
            };
            let mut display_name = name.clone();
            let mut child = child;

            // A directory with no files and exactly one directory child adds
            // no branching information, so show that chain as a single path.
            // Stop at a collapsed directory so its existing state remains
            // visible and meaningful when changing between tree modes.
            while compact_directories
                && !collapsed_directories.contains(&path)
                && child.files.is_empty()
                && child.directories.len() == 1
            {
                let (child_name, next_child) = child
                    .directories
                    .iter()
                    .next()
                    .expect("a single directory child must exist");
                path.push('/');
                path.push_str(child_name);
                display_name.push('/');
                display_name.push_str(child_name);
                child = next_child;
            }

            let collapsed = collapsed_directories.contains(&path);
            entries.push(FileTreeEntry::Directory {
                name: display_name,
                path: path.clone(),
                depth,
                collapsed,
            });
            if !collapsed {
                append_entries(
                    child,
                    &path,
                    depth + 1,
                    collapsed_directories,
                    compact_directories,
                    entries,
                );
            }
        }
        entries.extend(
            node.files
                .iter()
                .copied()
                .map(|index| FileTreeEntry::File { index, depth }),
        );
    }

    let mut entries = Vec::new();
    append_entries(
        &root,
        "",
        0,
        collapsed_directories,
        compact_directories,
        &mut entries,
    );
    entries
}

#[derive(Clone, Copy)]
pub(super) struct ReviewTab {
    pub(super) file_index: usize,
    pub(super) is_preview: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LogTab {
    Lsp,
}

impl LogTab {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Lsp => "LSP Logs",
        }
    }
}

/// Keep tabs that still identify a changed file after a workspace refresh.
/// File indexes are intentionally remapped by path because the diff list can
/// be reordered or lose entries after an edit, checkout, or commit.
pub(super) fn remap_tabs_after_refresh(
    previous_files: &[model::NativeFile],
    previous_tabs: &[ReviewTab],
    previous_selected: usize,
    refreshed_files: &[model::NativeFile],
) -> (Vec<ReviewTab>, usize) {
    let selected_path = previous_files
        .get(previous_selected)
        .map(|file| file.path.as_str());
    let mut tabs = previous_tabs
        .iter()
        .filter_map(|tab| {
            let path = previous_files.get(tab.file_index)?.path.as_str();
            let file_index = refreshed_files.iter().position(|file| file.path == path)?;
            Some(ReviewTab {
                file_index,
                is_preview: tab.is_preview,
            })
        })
        .collect::<Vec<_>>();

    let selected = selected_path
        .and_then(|path| refreshed_files.iter().position(|file| file.path == path))
        .or_else(|| tabs.first().map(|tab| tab.file_index))
        .unwrap_or(0);

    if tabs.is_empty() && !refreshed_files.is_empty() {
        tabs.push(ReviewTab {
            file_index: selected,
            is_preview: true,
        });
    }

    (tabs, selected)
}
