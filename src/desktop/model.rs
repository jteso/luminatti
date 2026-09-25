use super::semantic::{analyze_typescript, FileSemanticAnalysis};
use crate::command::diff::diff_algo::compute_side_by_side;
use crate::command::diff::types::{ChangeType, FileDiff, FileStatus, InlineSegment};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct NativeFile {
    pub path: String,
    pub status: FileStatus,
    pub additions: usize,
    pub deletions: usize,
    pub review_fingerprint: String,
    pub rows: Arc<[NativeRow]>,
    pub max_old_chars: usize,
    pub max_new_chars: usize,
    pub line_number_digits: usize,
    pub new_content: Arc<str>,
    pub semantic: Arc<FileSemanticAnalysis>,
    pub observed_at_millis: u128,
}

#[derive(Clone)]
pub struct NativeRow {
    pub old_number: Option<usize>,
    pub old_text: String,
    pub new_number: Option<usize>,
    pub new_text: String,
    pub change: ChangeType,
    pub old_segments: Option<Vec<InlineSegment>>,
    pub new_segments: Option<Vec<InlineSegment>>,
}

impl NativeFile {
    /// A sidebar-only record. Full source text is loaded only for the active file.
    pub fn summary(path: String, status: FileStatus) -> Self {
        Self {
            path,
            status,
            additions: 0,
            deletions: 0,
            review_fingerprint: String::new(),
            rows: Arc::from(Vec::<NativeRow>::new()),
            max_old_chars: 0,
            max_new_chars: 0,
            line_number_digits: 2,
            new_content: Arc::from(""),
            semantic: Arc::new(FileSemanticAnalysis::default()),
            observed_at_millis: 0,
        }
    }

    pub fn from_diff(diff: &FileDiff) -> Self {
        let rows = if diff.is_binary {
            Vec::new()
        } else {
            compute_side_by_side(&diff.old_content, &diff.new_content, 4)
                .into_iter()
                .map(|line| NativeRow {
                    old_number: line.old_line.as_ref().map(|(n, _)| *n),
                    old_text: line.old_line.map(|(_, text)| text).unwrap_or_default(),
                    new_number: line.new_line.as_ref().map(|(n, _)| *n),
                    new_text: line.new_line.map(|(_, text)| text).unwrap_or_default(),
                    change: line.change_type,
                    old_segments: line.old_segments,
                    new_segments: line.new_segments,
                })
                .collect()
        };
        let (added, removed, max_old_chars, max_new_chars, max_line_number) = rows.iter().fold(
            (0usize, 0usize, 0usize, 0usize, 0usize),
            |(added, removed, old_width, new_width, number), row: &NativeRow| {
                use ChangeType::{Delete, Insert, Modified};
                let (added, removed) = match row.change {
                    Insert => (added + 1, removed),
                    Delete => (added, removed + 1),
                    Modified => (added + 1, removed + 1),
                    ChangeType::Equal => (added, removed),
                };
                (
                    added,
                    removed,
                    old_width.max(row.old_text.chars().count()),
                    new_width.max(row.new_text.chars().count()),
                    number
                        .max(row.old_number.unwrap_or(0))
                        .max(row.new_number.unwrap_or(0)),
                )
            },
        );
        let (additions, deletions) = if diff.is_binary {
            (0, 0)
        } else {
            (added, removed)
        };
        let semantic = analyze_typescript(diff);
        Self {
            path: diff.filename.clone(),
            status: diff.status,
            additions,
            deletions,
            review_fingerprint: review_fingerprint(diff),
            rows: Arc::from(rows),
            max_old_chars,
            max_new_chars,
            line_number_digits: max_line_number.to_string().len().max(2),
            new_content: Arc::from(diff.new_content.as_str()),
            semantic: Arc::new(semantic),
            observed_at_millis: 0,
        }
    }

    pub fn with_observed_at(mut self, observed_at_millis: u128) -> Self {
        self.observed_at_millis = observed_at_millis;
        self
    }
}

fn review_fingerprint(diff: &FileDiff) -> String {
    let mut hasher = Sha256::new();
    hasher.update(diff.status.symbol().as_bytes());
    hasher.update([0]);
    hasher.update(diff.old_content.as_bytes());
    hasher.update([0]);
    hasher.update(diff.new_content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    pub file: String,
    pub line: usize,
    pub body: String,
}

pub struct ReviewModel {
    pub files: Vec<NativeFile>,
    pub selected: usize,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Copy, Default)]
pub struct ReviewSummary {
    pub typescript_files: usize,
    pub changed_symbols: usize,
    pub parse_errors: usize,
}

impl ReviewModel {
    pub fn new(files: Vec<NativeFile>, focus: Option<&str>) -> Self {
        let selected = focus
            .and_then(|path| files.iter().position(|file| file.path == path))
            .unwrap_or(0);
        Self {
            files,
            selected,
            annotations: load_annotations().unwrap_or_default(),
        }
    }

    pub fn selected_file(&self) -> Option<&NativeFile> {
        self.files.get(self.selected)
    }

    pub fn add_annotation(&mut self) {
        if let Some(file) = self.selected_file() {
            let line = file
                .rows
                .iter()
                .find_map(|row| row.new_number.or(row.old_number))
                .unwrap_or(1);
            self.annotations.push(Annotation {
                file: file.path.clone(),
                line,
                body: "Review this change with the AI harness.".to_string(),
            });
        }
    }

    pub fn persist_annotations(&self) -> io::Result<()> {
        save_annotations(&self.annotations)
    }

    pub fn replace_files(&mut self, files: Vec<NativeFile>) {
        self.files = files;
    }

    pub fn review_summary_for_indices(&self, indices: &[usize]) -> ReviewSummary {
        let files = indices
            .iter()
            .filter_map(|index| self.files.get(*index))
            .collect::<Vec<_>>();
        let typescript_files = files
            .iter()
            .filter(|file| !file.semantic.symbols.is_empty() || !file.semantic.changes.is_empty())
            .count();
        let changed_symbols = files.iter().map(|file| file.semantic.changes.len()).sum();
        let parse_errors = files.iter().map(|file| file.semantic.parse_errors).sum();
        ReviewSummary {
            typescript_files,
            changed_symbols,
            parse_errors,
        }
    }
}

fn annotations_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|directory| directory.join("luminatti").join("review-annotations.json"))
}

fn load_annotations() -> io::Result<Vec<Annotation>> {
    let Some(path) = annotations_path() else {
        return Ok(Vec::new());
    };
    match fs::read_to_string(path) {
        Ok(contents) => Ok(serde_json::from_str(&contents).unwrap_or_default()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn save_annotations(annotations: &[Annotation]) -> io::Result<()> {
    let Some(path) = annotations_path() else {
        return Ok(());
    };
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let contents = serde_json::to_string_pretty(annotations).map_err(io::Error::other)?;
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn keeps_paired_modified_rows() {
        let file = NativeFile::from_diff(&FileDiff {
            filename: "src/app.rs".into(),
            old_content: "let a = 1;\n".into(),
            new_content: "let a = 2;\n".into(),
            status: FileStatus::Modified,
            is_binary: false,
        });
        assert_eq!(file.rows.len(), 1);
        assert!(matches!(file.rows[0].change, ChangeType::Modified));
    }

    #[test]
    fn creating_a_model_does_not_create_repository_metadata() {
        let repository = TempDir::new().unwrap();

        let _ = ReviewModel::new(Vec::new(), None);

        assert!(!repository.path().join(".luminatti").exists());
    }

    #[test]
    fn review_summary_can_be_scoped_to_visible_files() {
        let older = NativeFile::from_diff(&FileDiff {
            filename: "src/older.ts".into(),
            old_content: "function value() { return 1; }".into(),
            new_content: "function value() { return 2; }".into(),
            status: FileStatus::Modified,
            is_binary: false,
        })
        .with_observed_at(10);
        let newer = NativeFile::from_diff(&FileDiff {
            filename: "src/newer.ts".into(),
            old_content: "function before() { return 1; }".into(),
            new_content: "function after() { return 1; }".into(),
            status: FileStatus::Modified,
            is_binary: false,
        })
        .with_observed_at(20);
        let model = ReviewModel::new(vec![older, newer], None);

        let filtered_summary = model.review_summary_for_indices(&[0]);
        assert_eq!(
            filtered_summary.changed_symbols,
            model.files[0].semantic.changes.len()
        );
        assert_eq!(filtered_summary.typescript_files, 1);
        assert_eq!(filtered_summary.parse_errors, 0);
    }
}
