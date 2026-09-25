//! Session-only review notes. Deliberately excluded from project settings and disk storage.
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LineRange {
    pub file: String,
    pub start: usize,
    pub end: usize,
}

impl LineRange {
    pub fn new(file: String, anchor: usize, head: usize) -> Self {
        Self {
            file,
            start: anchor.min(head),
            end: anchor.max(head),
        }
    }

    pub fn contains(&self, file: &str, line: usize) -> bool {
        self.file == file && (self.start..=self.end).contains(&line)
    }
}

#[derive(Clone, Debug)]
pub(super) struct CommentTarget {
    pub file: String,
    pub ranges: Vec<LineRange>,
}

impl From<LineRange> for CommentTarget {
    fn from(range: LineRange) -> Self {
        Self {
            file: range.file.clone(),
            ranges: vec![range],
        }
    }
}

impl CommentTarget {
    pub fn contains(&self, file: &str, line: usize) -> bool {
        self.ranges.iter().any(|range| range.contains(file, line))
    }

    pub fn location(&self) -> String {
        format!(
            "{}:{}",
            self.file,
            self.ranges
                .iter()
                .map(|range| {
                    if range.start == range.end {
                        range.start.to_string()
                    } else {
                        format!("{}-{}", range.start, range.end)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[derive(Default)]
pub(super) struct LineSelection {
    pub file: String,
    pub lines: BTreeSet<usize>,
    anchor: usize,
    drag_anchor: usize,
    drag_head: usize,
    drag_base: BTreeSet<usize>,
    pub dragging: bool,
}

impl LineSelection {
    pub fn click(&mut self, file: &str, line: usize, command: bool, shift: bool) {
        if self.file != file {
            *self = Self {
                file: file.into(),
                ..Self::default()
            };
        }
        let anchor = if shift && self.anchor > 0 {
            self.anchor
        } else {
            line
        };
        if !command {
            self.lines.clear();
        }
        self.drag_base = self.lines.clone();
        self.drag_anchor = anchor;
        self.drag_head = line;
        if command && !shift && self.lines.remove(&line) {
            self.drag_base = self.lines.clone();
        } else {
            self.lines.extend(anchor.min(line)..=anchor.max(line));
        }
        if !shift {
            self.anchor = line;
        }
        self.dragging = true;
    }

    pub fn drag_to(&mut self, file: &str, line: usize) -> bool {
        if !self.dragging || self.file != file || self.drag_head == line {
            return false;
        }
        self.drag_head = line;
        let mut lines = self.drag_base.clone();
        lines.extend(self.drag_anchor.min(line)..=self.drag_anchor.max(line));
        if lines == self.lines {
            return false;
        }
        self.lines = lines;
        true
    }

    pub fn target(&self) -> Option<CommentTarget> {
        let mut ranges: Vec<LineRange> = Vec::new();
        for line in &self.lines {
            if let Some(last) = ranges.last_mut().filter(|last| last.end + 1 == *line) {
                last.end = *line;
            } else {
                ranges.push(LineRange::new(self.file.clone(), *line, *line));
            }
        }
        (!ranges.is_empty()).then(|| CommentTarget {
            file: self.file.clone(),
            ranges,
        })
    }
}

#[derive(Clone, Debug)]
pub(super) struct Comment {
    pub target: CommentTarget,
    pub body: String,
}

#[derive(Default)]
pub(super) struct State {
    pub repositories: HashMap<PathBuf, Vec<Comment>>,
    pub active: bool,
    pub selection: LineSelection,
    pub context_menu: Option<(f32, f32)>,
    pub draft: Option<CommentTarget>,
    pub copied: bool,
    pub copy_epoch: u64,
}

impl State {
    pub fn add(&mut self, repository: PathBuf, body: &str) -> bool {
        let body = body.trim();
        if body.is_empty() || self.draft.is_none() {
            return false;
        }
        self.repositories
            .entry(repository)
            .or_default()
            .push(Comment {
                target: self.draft.take().unwrap(),
                body: body.to_string(),
            });
        self.selection = LineSelection::default();
        self.copied = false;
        true
    }

    pub fn reset_view(&mut self) {
        let repositories = std::mem::take(&mut self.repositories);
        // Invalidate a pending clipboard-feedback timer across repository switches.
        let copy_epoch = self.copy_epoch.wrapping_add(1);
        *self = Self {
            repositories,
            copy_epoch,
            ..Self::default()
        };
    }
}

pub(super) fn follow_up_prompt(comments: &[Comment]) -> String {
    if comments.is_empty() {
        return String::new();
    }
    let mut prompt = String::from(
        "After reviewing your changes, I have the following questions and feedback.\n\n\
         Please inspect the relevant code and address each numbered comment in order, referencing its file and line range. \
         Explain the reasoning behind the current implementation when answering questions. \
         Where I explicitly request a change, make the focused update and describe how you validated it. \
         If a comment is ambiguous, ask a focused follow-up question before changing behavior.\n\n\
         Line numbers refer to the working copy at the time of review; if the code has moved, locate the corresponding code first.\n\n\
         Review comments:\n",
    );
    for (index, comment) in comments.iter().enumerate() {
        prompt.push_str(&format!("\n{}. {}\n", index + 1, comment.target.location()));
        for line in comment.body.lines() {
            prompt.push_str("   ");
            prompt.push_str(line);
            prompt.push('\n');
        }
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upward_and_downward_drags_cover_the_same_inclusive_lines() {
        let range = LineRange::new("src/a.rs".into(), 12, 8);
        assert_eq!(range, LineRange::new("src/a.rs".into(), 8, 12));
        assert!(range.contains("src/a.rs", 8));
        assert!(range.contains("src/a.rs", 12));
        assert!(!range.contains("src/a.rs", 13));
        assert!(!range.contains("src/b.rs", 10));
    }

    #[test]
    fn command_click_and_shift_click_preserve_disjoint_ranges() {
        let mut selection = LineSelection::default();
        selection.click("a.rs", 8, false, false);
        selection.click("a.rs", 14, true, false);
        selection.click("a.rs", 18, true, true);
        assert_eq!(selection.target().unwrap().location(), "a.rs:8, 14-18");
        assert!(!selection.target().unwrap().contains("a.rs", 9));
        selection.click("a.rs", 14, true, false);
        assert_eq!(selection.target().unwrap().location(), "a.rs:8, 15-18");
        selection.click("a.rs", 20, false, true);
        assert_eq!(selection.target().unwrap().location(), "a.rs:14-20");
    }

    #[test]
    fn additive_drag_can_reverse_without_selecting_gaps() {
        let mut selection = LineSelection::default();
        selection.click("a.rs", 2, false, false);
        selection.click("a.rs", 10, true, false);
        selection.drag_to("a.rs", 7);
        assert_eq!(selection.target().unwrap().location(), "a.rs:2, 7-10");
        selection.drag_to("a.rs", 12);
        assert_eq!(selection.target().unwrap().location(), "a.rs:2, 10-12");
        assert!(!selection.drag_to("b.rs", 30));
        selection.click("b.rs", 3, true, true);
        assert_eq!(selection.target().unwrap().location(), "b.rs:3");
    }

    #[test]
    fn saving_reopens_the_tab_but_does_not_interrupt_an_open_review() {
        let mut state = State::default();
        let repo = PathBuf::from("/repo");
        state.draft = Some(LineRange::new("a.rs".into(), 3, 3).into());
        assert!(!state.add(repo.clone(), " \n "));
        assert!(state.draft.is_some());
        assert!(!state.open);
        assert!(state.add(repo.clone(), " Why? "));
        assert!(state.open && state.active);
        state.active = false;
        state.draft = Some(LineRange::new("a.rs".into(), 4, 6).into());
        assert!(state.add(repo.clone(), "Please simplify this."));
        assert!(!state.active);
        state.reset_view();
        assert_eq!(state.repositories[&repo].len(), 2);
        assert!(!state.repositories.contains_key(&PathBuf::from("/other")));
        assert!(!state.open);
    }

    #[test]
    fn clipboard_prompt_preserves_files_ranges_and_multiline_feedback() {
        let notes = vec![
            Comment {
                target: LineRange::new("src/a.rs".into(), 2, 2).into(),
                body: "Why return early?".into(),
            },
            Comment {
                target: LineRange::new("src/b.rs".into(), 9, 5).into(),
                body: "Please handle errors.\nKeep the original message: café 🦀".into(),
            },
        ];
        let prompt = follow_up_prompt(&notes);
        assert!(prompt.starts_with("After reviewing your changes"));
        assert!(prompt.contains("1. src/a.rs:2\n   Why return early?"));
        assert!(prompt.contains(
            "2. src/b.rs:5-9\n   Please handle errors.\n   Keep the original message: café 🦀"
        ));
        assert!(prompt.contains("ask a focused follow-up question"));
        assert!(follow_up_prompt(&[]).is_empty());
    }
}
