//! GPUI rendering components for the desktop review workspace.

use std::ops::Range;

use gpui::{pattern_slash, svg, uniform_list, AnyElement, Styled, StyledText};

use super::*;

const UNIFIED_LEFT_PAD: f32 = 3.;
const UNIFIED_COUNTER_GAP: f32 = 20.;
const UNIFIED_RIGHT_PAD: f32 = 3.;
const SPLIT_LEFT_PAD: f32 = 3.;
const SPLIT_RIGHT_PAD: f32 = 5.;

/// One item per visible line keeps the unified list at a fixed row height.
enum UnifiedDisplayRow {
    Code { source_row: usize, old_side: bool },
    Collapsed(UnchangedSection),
}

fn middle_ellipsize(name: &str, max_chars: usize) -> String {
    let chars = name.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return name.to_string();
    }
    let visible = max_chars.saturating_sub(1);
    let start = visible / 2;
    let end = visible - start;
    format!(
        "{}…{}",
        chars[..start].iter().collect::<String>(),
        chars[chars.len() - end..].iter().collect::<String>()
    )
}

/// GPUI normally maps a wheel's vertical delta to a horizontal-only scroll
/// container. Diff rows nest those containers inside the vertical diff list,
/// so opt into the browser-like axis behavior for code panes.
fn restrict_wheel_scroll_to_matching_axis<E: Styled>(mut element: E) -> E {
    element.style().restrict_scroll_to_axis = Some(true);
    element
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReviewChangeFamily {
    #[serde(alias = "implementation")]
    Behaviour,
    Contract,
    Dependency,
    #[serde(alias = "structural", alias = "renamed", alias = "moved")]
    Structure,
    Configuration,
    Data,
    Test,
    Documentation,
}

impl ReviewChangeFamily {
    const ALL: [Self; 8] = [
        Self::Behaviour,
        Self::Contract,
        Self::Dependency,
        Self::Structure,
        Self::Configuration,
        Self::Data,
        Self::Test,
        Self::Documentation,
    ];

    /// A file can have more than one change family. For example, a changed
    /// TypeScript function may alter behaviour and its public contract.
    pub(super) fn for_file(file: &super::model::NativeFile) -> Vec<Self> {
        let path = file.path.to_ascii_lowercase();
        let mut families = Vec::new();

        if is_documentation_path(&path) {
            families.push(Self::Documentation);
        }
        if is_test_path(&path) {
            families.push(Self::Test);
        }
        if is_configuration_path(&path) {
            families.push(Self::Configuration);
        }
        if is_data_path(&path) {
            families.push(Self::Data);
        }
        if is_dependency_manifest(&path) || has_changed_import(file) {
            families.push(Self::Dependency);
        }

        use semantic::SemanticChangeKind::*;
        for change in &file.semantic.changes {
            match change.kind {
                ImplementationChanged => families.push(Self::Behaviour),
                SignatureChanged => families.push(Self::Contract),
                Added | Removed | Renamed | Moved => families.push(Self::Structure),
            }
        }

        families.sort_unstable();
        families.dedup();
        families
    }

    fn label(self) -> &'static str {
        match self {
            Self::Behaviour => "Behaviour",
            Self::Contract => "Contract",
            Self::Dependency => "Dependency",
            Self::Structure => "Structure",
            Self::Configuration => "Configuration",
            Self::Data => "Data",
            Self::Test => "Test",
            Self::Documentation => "Documentation",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::Behaviour => "Behaviour",
            Self::Contract => "Contract",
            Self::Dependency => "Dependency",
            Self::Structure => "Structure",
            Self::Configuration => "Config",
            Self::Data => "Data",
            Self::Test => "Test",
            Self::Documentation => "Docs",
        }
    }

    fn accent(self) -> u32 {
        match self {
            Self::Behaviour => 0xb29be8,
            Self::Contract => BLUE,
            Self::Dependency => 0xe5b567,
            Self::Structure => 0x78b5d8,
            Self::Configuration => 0xd596d5,
            Self::Data => 0x5fc6b6,
            Self::Test => GREEN,
            Self::Documentation => 0x9da7b6,
        }
    }
}

fn is_documentation_path(path: &str) -> bool {
    path.starts_with("docs/")
        || path.contains("/docs/")
        || matches!(
            path.rsplit('/').next().unwrap_or(path),
            "readme" | "readme.md" | "changelog" | "changelog.md" | "contributing.md"
        )
        || path.ends_with(".md")
        || path.ends_with(".mdx")
        || path.ends_with(".rst")
        || path.ends_with(".adoc")
}

fn is_test_path(path: &str) -> bool {
    path.starts_with("test/")
        || path.starts_with("tests/")
        || path.contains("/test/")
        || path.contains("/tests/")
        || path.contains("/__tests__/")
        || path.contains("/spec/")
        || path.contains("/specs/")
        || path.contains(".test.")
        || path.contains(".spec.")
        || path.ends_with("_test.rs")
        || path.ends_with("_test.go")
}

fn is_configuration_path(path: &str) -> bool {
    path.starts_with(".env")
        || path.contains("/config/")
        || path.contains("/configs/")
        || matches!(
            path.rsplit('/').next().unwrap_or(path),
            "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | "makefile" | "tsconfig.json"
                | "eslint.config.js" | "eslint.config.mjs" | "prettier.config.js"
        )
        || path.ends_with(".yml")
        || path.ends_with(".yaml")
        || path.ends_with(".toml")
        || path.ends_with(".ini")
        || path.ends_with(".conf")
        || path.ends_with(".properties")
}

fn is_data_path(path: &str) -> bool {
    path.starts_with("data/")
        || path.contains("/data/")
        || path.starts_with("migrations/")
        || path.contains("/migrations/")
        || path.contains("/fixtures/")
        || path.ends_with(".sql")
        || path.ends_with(".csv")
        || path.ends_with(".tsv")
        || path.ends_with(".prisma")
}

fn is_dependency_manifest(path: &str) -> bool {
    matches!(
        path.rsplit('/').next().unwrap_or(path),
        "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "cargo.toml"
            | "cargo.lock" | "go.mod" | "go.sum" | "requirements.txt" | "poetry.lock"
            | "pyproject.toml" | "gemfile" | "gemfile.lock" | "composer.json" | "composer.lock"
    )
}

fn has_changed_import(file: &super::model::NativeFile) -> bool {
    file.rows.iter().any(|row| {
        !matches!(row.change, crate::command::diff::types::ChangeType::Equal)
            && [row.old_text.as_str(), row.new_text.as_str()]
                .into_iter()
                .map(str::trim_start)
                .any(|line| {
                    line.starts_with("import ")
                        || line.starts_with("export ")
                        || line.starts_with("use ")
                        || line.starts_with("require(")
                })
    })
}

pub(super) struct ReviewTooltip(pub String);

impl Render for ReviewTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(520.))
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(BG))
            .text_size(px(11.))
            .text_color(rgb(TEXT))
            .child(self.0.clone())
    }
}

impl ReviewWorkspace {
    pub(super) fn review_checkbox(checked: bool) -> impl IntoElement {
        div()
            .size(px(14.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .border_1()
            .border_color(rgb(if checked { 0x859cff } else { 0x606a79 }))
            .bg(rgb(if checked { 0x667cf5 } else { PANEL }))
            .text_size(px(11.))
            .text_color(rgb(0xffffff))
            .child(if checked { "✓" } else { "" })
    }

    fn syntax_color(class: crate::command::diff::highlight::SyntaxClass) -> u32 {
        use crate::command::diff::highlight::SyntaxClass;
        match class {
            SyntaxClass::Comment => 0x7f8c98,
            SyntaxClass::Keyword => 0xc792ea,
            SyntaxClass::String => 0xa8cc8c,
            SyntaxClass::Number | SyntaxClass::Constant => 0xf2b482,
            SyntaxClass::Function => 0x82aaff,
            SyntaxClass::Type => 0xffcb6b,
            SyntaxClass::Property => 0x89ddff,
            SyntaxClass::Operator | SyntaxClass::Punctuation => 0x9da7b6,
            SyntaxClass::Tag => 0xf18c96,
            SyntaxClass::Attribute => 0xc3e88d,
            SyntaxClass::Variable => 0xc9d1d9,
        }
    }

    fn syntax_text_with_emphasis(
        text: String,
        ranges: &[crate::command::diff::highlight::SyntaxRange],
        segments: Option<&[crate::command::diff::types::InlineSegment]>,
        emphasis_bg: u32,
    ) -> StyledText {
        let syntax = ranges.iter().filter(|span| {
            span.range.end <= text.len()
                && text.is_char_boundary(span.range.start)
                && text.is_char_boundary(span.range.end)
        }).collect::<Vec<_>>();
        let mut emphasis = Vec::new();
        let mut offset = 0;
        if let Some(segments) = segments {
            for segment in segments {
                let end = offset + segment.text.len();
                if segment.emphasized && end <= text.len()
                    && text.is_char_boundary(offset) && text.is_char_boundary(end) {
                    emphasis.push(offset..end);
                }
                offset = end;
            }
        }
        let mut boundaries = vec![0, text.len()];
        for span in &syntax {
            boundaries.extend([span.range.start, span.range.end]);
        }
        for range in &emphasis {
            boundaries.extend([range.start, range.end]);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut highlights: Vec<(Range<usize>, gpui::HighlightStyle)> = Vec::new();
        for pair in boundaries.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let color = syntax.iter().find(|span| span.range.start <= start && start < span.range.end)
                .map(|span| rgb(Self::syntax_color(span.class)).into());
            let changed = emphasis.iter().any(|range| range.start <= start && start < range.end);
            if color.is_none() && !changed { continue; }
            let style = gpui::HighlightStyle {
                color,
                background_color: changed.then(|| rgb(emphasis_bg).into()),
                ..Default::default()
            };
            if let Some((previous, previous_style)) = highlights.last_mut() {
                if previous.end == start && *previous_style == style {
                    previous.end = end;
                    continue;
                }
            }
            highlights.push((start..end, style));
        }
        StyledText::new(text).with_highlights(highlights)
    }

    fn folder_icon(open: bool) -> impl IntoElement {
        let asset = if open {
            "icons/folder_open.svg"
        } else {
            "icons/folder.svg"
        };

        svg()
            .path(asset)
            .size(px(16.))
            .flex_none()
            .text_color(rgb(MUTED))
    }

    fn change_arrow_icon(up: bool, enabled: bool) -> impl IntoElement {
        let asset = if up {
            "icons/arrow_up.svg"
        } else {
            "icons/arrow_down.svg"
        };

        svg()
            .path(asset)
            .size(px(16.))
            .flex_none()
            .text_color(if enabled { rgb(TEXT) } else { rgb(MUTED) })
    }

    fn unchanged_context_icon(all_sections_collapsed: bool) -> impl IntoElement {
        let asset = if all_sections_collapsed {
            "icons/expand_unchanged.svg"
        } else {
            "icons/collapse_unchanged.svg"
        };
        svg()
            .path(asset)
            .size(px(16.))
            .flex_none()
            .text_color(rgb(MUTED))
    }

    /// Zed's paired diff icons, rendered as native elements so they remain
    /// visible in the desktop renderer. The icon shows the view a click will
    /// select, rather than the active view.
    fn diff_view_icon(target: DiffView) -> AnyElement {
        match target {
            DiffView::Split => div()
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(2.))
                .child(
                    div()
                        .w(px(5.))
                        .h(px(11.))
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(MUTED)),
                )
                .child(
                    div()
                        .w(px(5.))
                        .h(px(11.))
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(MUTED)),
                )
                .into_any_element(),
            DiffView::Unified => div()
                .size(px(16.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(2.))
                .child(
                    div()
                        .w(px(12.))
                        .h(px(5.))
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(MUTED)),
                )
                .child(
                    div()
                        .w(px(12.))
                        .h(px(5.))
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(MUTED)),
                )
                .into_any_element(),
        }
    }

    pub(super) fn sidebar_toggle_icon(sidebar_is_open: bool) -> impl IntoElement {
        let asset = if sidebar_is_open {
            "icons/threads_sidebar_left_open.svg"
        } else {
            "icons/threads_sidebar_left_closed.svg"
        };

        svg()
            .path(asset)
            .size(px(14.))
            .flex_none()
            .text_color(rgb(TEXT))
    }

    pub(super) fn file_filter_icon(color: u32) -> impl IntoElement {
        svg()
            .path("icons/filter.svg")
            .size(px(14.))
            .flex_none()
            .text_color(rgb(color))
    }

    pub(super) fn file_view_settings_icon(color: u32) -> impl IntoElement {
        svg()
            .path("icons/settings.svg")
            .size(px(16.))
            .flex_none()
            .text_color(rgb(color))
    }

    /// Render the guide for every ancestor of a tree row. This mirrors Zed's
    /// project panel approach: guides run across consecutive descendant rows
    /// and naturally end when the tree returns to a shallower depth.
    fn tree_indent_guides(depth: usize, ending_guides: &[usize]) -> Vec<AnyElement> {
        const ROW_LEFT_PADDING: f32 = 10.;
        const ICON_CENTER_OFFSET: f32 = 8.;
        const INDENT_WIDTH: f32 = 14.;

        (0..depth)
            .map(|level| {
                div()
                    .absolute()
                    .top_0()
                    // End a guide at the final row's text baseline rather
                    // than extending it through the row's bottom padding.
                    .bottom(if ending_guides.contains(&level) {
                        px(8.)
                    } else {
                        px(0.)
                    })
                    .left(px(ROW_LEFT_PADDING
                        + ICON_CENTER_OFFSET
                        + level as f32 * INDENT_WIDTH))
                    .w(px(1.))
                    .bg(rgb(TREE_INDENT_GUIDE))
                    .into_any_element()
            })
            .collect()
    }

    pub(super) fn file_row(
        &self,
        file_entry: SidebarFileEntry,
        depth: usize,
        show_full_path: bool,
        ending_guides: &[usize],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let file = file_entry
            .model_index
            .and_then(|index| self.model.files.get(index));
        let selected = file_entry
            .model_index
            .is_some_and(|index| self.is_file_selected(index));
        let label = if show_full_path {
            file_entry.path.clone()
        } else {
            file_entry
                .path
                .rsplit('/')
                .next()
                .unwrap_or(file_entry.path.as_str())
                .to_string()
        };
        let symbol = file.map(|file| file.status.symbol()).unwrap_or_default();
        let additions = file.map(|file| file.additions).unwrap_or_default();
        let deletions = file.map(|file| file.deletions).unwrap_or_default();
        let is_changed = file_entry.is_changed;
        let reviewed = self.reviewed_files.contains(&file_entry.path);
        let path = file_entry.path;
        let review_path = path.clone();
        div()
            .id(gpui::SharedString::from(format!("file-{path}")))
            .w_full()
            .pl(px(10. + depth as f32 * 14.))
            .h(px(22.))
            .relative()
            .flex()
            .gap_1()
            .items_center()
            .cursor_pointer()
            .rounded_sm()
            .text_size(px(11.))
            .text_color(if selected {
                rgb(TEXT)
            } else if is_changed {
                rgb(MUTED)
            } else {
                rgb(0x778295)
            })
            .bg(if selected {
                rgb(ACTIVE_FILE_BG)
            } else {
                rgb(PANEL)
            })
            .hover(move |element| {
                element.bg(if selected {
                    rgb(ACTIVE_FILE_HOVER_BG)
                } else {
                    rgb(0x2c3443)
                })
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                this.choose_sidebar_file(path.clone(), event.click_count() >= 2, cx)
            }))
            .children(Self::tree_indent_guides(depth, ending_guides))
            .child(
                div()
                    .w(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(is_changed, |element| {
                        element
                            .text_color(match symbol {
                                "A" => rgb(GREEN),
                                "D" => rgb(RED),
                                _ => rgb(BLUE),
                            })
                            .child(symbol)
                    }),
            )
            .child(div().flex_1().truncate().child(label))
            .when(is_changed, |element| {
                element
                    .when(!reviewed, |element| element
                        .child(div().text_size(px(11.)).text_color(rgb(GREEN)).child(format!("+{additions}")))
                        .child(div().text_size(px(11.)).text_color(rgb(RED)).child(format!("-{deletions}"))))
                    .when(reviewed, |element| element.child(div().text_size(px(11.))
                        .text_color(rgb(MUTED)).child("Reviewed")))
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!("review-file-{review_path}")))
                            .w(px(24.))
                            .h_full()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .tooltip(move |_, cx| cx.new(|_| ReviewTooltip(if reviewed { "Mark file unreviewed" } else { "Mark file reviewed" }.into())).into())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_file_reviewed(&review_path, cx);
                            }))
                            .child(Self::review_checkbox(reviewed)),
                    )
            })
    }

    fn directory_row(
        &self,
        name: String,
        path: String,
        depth: usize,
        collapsed: bool,
        ending_guides: &[usize],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(gpui::SharedString::from(format!("directory-{path}")))
            .w_full()
            .pl(px(10. + depth as f32 * 14.))
            .h(px(22.))
            .relative()
            .flex()
            .gap_1()
            .items_center()
            .cursor_pointer()
            .rounded_sm()
            .text_size(px(11.))
            .text_color(rgb(TEXT))
            .hover(|element| element.bg(rgb(0x2c3443)))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_directory(path.clone(), cx)))
            .children(Self::tree_indent_guides(depth, ending_guides))
            .child(Self::folder_icon(!collapsed))
            .child(div().truncate().child(name))
    }

    pub(super) fn change_type_filter_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scoped_indices = self.filtered_file_indices();
        let summary = self.model.review_summary_for_indices(&scoped_indices);
        let stats = ReviewChangeFamily::ALL.map(|family| {
            let mut files = 0usize;
            for index in &scoped_indices {
                let Some(file) = self.model.files.get(*index) else {
                    continue;
                };
                files += usize::from(ReviewChangeFamily::for_file(file).contains(&family));
            }
            (family, files)
        });
        let active_count = self.change_type_filters.len();
        let scope_tooltip = format!(
            "{} TypeScript symbol {} across {} {}.{}\nChange families use TypeScript structure and file-path evidence. A file can belong to more than one family.",
            summary.changed_symbols,
            if summary.changed_symbols == 1 { "change" } else { "changes" },
            summary.typescript_files,
            if summary.typescript_files == 1 { "file" } else { "files" },
            if summary.parse_errors == 0 {
                String::new()
            } else {
                format!(" {} parse errors were detected.", summary.parse_errors)
            },
        );
        let segments =
            stats
                .iter()
                .enumerate()
                .filter(|(_, (_, files))| *files > 0)
                .map(|(index, (family, files))| {
                    let family = *family;
                    let accent = family.accent();
                    let selected = self.change_type_filters.contains(&family);
                    let dimmed = active_count > 0 && !selected;
                    let tooltip = format!(
                        "{}: {} {}\nClick to {} this file filter.",
                        family.label(),
                        files,
                        if *files == 1 { "file" } else { "files" },
                        if selected { "remove" } else { "add" },
                    );
                    div()
                        .id(("change-type-segment", index))
                        .h_full()
                        .flex_grow()
                        .cursor_pointer()
                        .bg(rgb(if dimmed { 0x4b5260 } else { accent }))
                        .hover(move |element| element.bg(rgb(accent)))
                        .tooltip(move |_, cx| cx.new(|_| ReviewTooltip(tooltip.clone())).into())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_change_type_filter(family, cx)
                        }))
                });
        let legend = stats
            .into_iter()
            .enumerate()
            .map(|(index, (family, files))| {
                let selected = self.change_type_filters.contains(&family);
                let accent = family.accent();
                let tooltip = format!(
                    "{}: {} {}",
                    family.label(),
                    files,
                    if files == 1 { "file" } else { "files" },
                );
                div()
                    .id(("change-type-legend", index))
                    .h(px(22.))
                    .flex_none()
                    .px_1()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .rounded_sm()
                    .bg(if selected { rgb(0x3d485a) } else { rgb(PANEL) })
                    .text_size(px(10.))
                    .text_color(rgb(if files == 0 { 0x697384 } else { TEXT }))
                    .tooltip(move |_, cx| cx.new(|_| ReviewTooltip(tooltip.clone())).into())
                    .when(files > 0, |element| {
                        element
                            .cursor_pointer()
                            .hover(|element| element.bg(rgb(0x384253)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_change_type_filter(family, cx)
                            }))
                    })
                    .child(div().size(px(6.)).rounded_sm().bg(rgb(accent)))
                    .child(div().truncate().child(family.short_label()))
                    .child(files.to_string())
            });

        div()
            .w_full()
            .pb_2()
            .flex()
            .flex_col()
            .gap_1()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("change-type-heading")
                            .flex_1()
                            .text_size(px(11.))
                            .text_color(rgb(TEXT))
                            .tooltip(move |_, cx| {
                                cx.new(|_| ReviewTooltip(scope_tooltip.clone())).into()
                            })
                            .child("Summary"),
                    )
                    .when(active_count > 0, |element| {
                        element.child(
                            div()
                                .id("clear-change-type-filters")
                                .cursor_pointer()
                                .text_size(px(10.))
                                .text_color(rgb(BLUE))
                                .hover(|element| element.text_color(rgb(TEXT)))
                                .on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.clear_change_type_filters(cx)
                                    }),
                                )
                                .child(format!("{active_count} active · Clear")),
                        )
                    }),
            )
            .child(
                div()
                    .px_2()
                    .child(
                        div()
                            .id("change-type-bar")
                            .h(px(10.))
                            .w_full()
                            .flex()
                            .overflow_hidden()
                            .rounded_sm()
                            .bg(rgb(0x242b36))
                            .children(segments),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(8.))
                    .gap_y(px(2.))
                    .children(legend),
            )
    }

    pub(super) fn file_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let visible_files = self.sidebar_file_entries();
        let entries: Vec<AnyElement> = match self.file_view {
            FileView::Flat => visible_files
                .iter()
                .cloned()
                .map(|file| self.file_row(file, 0, true, &[], cx).into_any_element())
                .collect(),
            FileView::Tree | FileView::CompactTree => {
                let visible_paths = visible_files
                    .iter()
                    .map(|file| file.path.clone())
                    .collect::<Vec<_>>();
                let entries = build_file_tree_entries(
                    &visible_paths,
                    &self.collapsed_directories,
                    self.file_view == FileView::CompactTree,
                );
                let ending_guides = entries
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let next_depth = entries
                            .get(index + 1)
                            .map(FileTreeEntry::depth)
                            .unwrap_or(0);
                        (next_depth.min(entry.depth())..entry.depth()).collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                entries
                    .into_iter()
                    .zip(ending_guides)
                    .map(|(entry, ending_guides)| match entry {
                        FileTreeEntry::Directory {
                            name,
                            path,
                            depth,
                            collapsed,
                        } => self
                            .directory_row(name, path, depth, collapsed, &ending_guides, cx)
                            .into_any_element(),
                        FileTreeEntry::File { index, depth } => self
                            .file_row(
                                visible_files[index].clone(),
                                depth,
                                false,
                                &ending_guides,
                                cx,
                            )
                            .into_any_element(),
                    })
                    .collect()
            }
        };
        div().flex().flex_col().children(entries)
    }

    fn diff_row(
        &self,
        file_path: &str,
        row: &NativeRow,
        index: usize,
        fold: Option<UnchangedSection>,
        old_syntax: Vec<crate::command::diff::highlight::SyntaxRange>,
        new_syntax: Vec<crate::command::diff::highlight::SyntaxRange>,
        line_number_width: Pixels,
        left_width: Pixels,
        left_content_width: Pixels,
        right_content_width: Pixels,
        left_code_scroll: ScrollHandle,
        right_code_scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Leave missing lines transparent so the continuous gap hatch shows through.
        let left_gap_marker = row.old_number.is_none() && row.new_number.is_some();
        let right_gap_marker = row.new_number.is_none() && row.old_number.is_some();
        let (left_bg, right_bg) = match row.change {
            crate::command::diff::types::ChangeType::Equal => (BG, BG),
            crate::command::diff::types::ChangeType::Delete => (0x35292f, BG),
            crate::command::diff::types::ChangeType::Insert => (BG, 0x253a32),
            crate::command::diff::types::ChangeType::Modified => (0x35292f, 0x253a32),
        };
        let highlighted = self.comment_highlight(file_path, row.new_number);
        let selected = self.line_is_selected(file_path, row.new_number);
        let right_code_bg = if selected {
            if matches!(row.change, crate::command::diff::types::ChangeType::Insert | crate::command::diff::types::ChangeType::Modified) {
                0x355747
            } else {
                0x3b4d68
            }
        } else if highlighted {
            super::comments_view::COMMENT_BG
        } else {
            right_bg
        };
        let line = row.new_number;
        let select_path = file_path.to_string();
        let drag_path = file_path.to_string();
        let menu_path = file_path.to_string();
        let left_fold = fold.clone();
        let right_fold = fold;
        div()
            .id(("row", index))
            .flex()
            .w_full()
            // `uniform_list` derives the scrollable content height from its first
            // rendered item. A collapsed unchanged section can be that first item,
            // so every diff row needs a concrete, matching height rather than a
            // minimum height whose measurement depends on its children.
            .h(px(22.))
            .font_family("JetBrains Mono")
            .text_size(px(12.))
            .child(
                div()
                    .w(left_width)
                    .when(!left_gap_marker, |element| element.bg(rgb(BG)))
                    .flex()
                    .relative()
                    .overflow_hidden()
                    .child(
                        div()
                            .w(px(SPLIT_LEFT_PAD))
                            .flex_none()
                            .flex()
                            .items_center()
                            .child(if let Some(section) = left_fold {
                                Self::expanded_fold_icon(section, px(SPLIT_LEFT_PAD), cx).into_any_element()
                            } else {
                                div().into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .w(line_number_width)
                            .flex_none()
                            .text_right()
                            .text_color(rgb(0x70798a))
                            .child(row.old_number.map(|n| n.to_string()).unwrap_or_default()),
                    )
                    .child(div().w(px(SPLIT_RIGHT_PAD)).flex_none())
                    .child(
                        restrict_wheel_scroll_to_matching_axis(
                            div()
                                .id(("old-code", index))
                                .flex_1()
                                .when(!left_gap_marker, |element| element.bg(rgb(left_bg)))
                                .overflow_x_scroll()
                                .track_scroll(&left_code_scroll),
                        )
                            .child(
                                div()
                                    .min_w(left_content_width)
                                    .whitespace_nowrap()
                                    .text_color(rgb(TEXT))
                                    .child(Self::syntax_text_with_emphasis(
                                        row.old_text.clone(), &old_syntax,
                                        row.old_segments.as_deref(), 0x713641,
                                    )),
                            ),
                    ),
            )
            .child(div().w(px(2.)).bg(rgb(BORDER)))
            .child(
                div()
                    .flex_1()
                    .id(("working-copy-line", index))
                    .when(!right_gap_marker, |element| element.bg(rgb(BG)))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, event, _, cx| this.select_comment_line(&select_path, line, event, cx)))
                    .on_mouse_move(cx.listener(move |this, event, _, cx| this.extend_comment_selection(&drag_path, line, event, cx)))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |this, event, window, cx| this.show_comment_context_menu(&menu_path, line, event, window, cx)))
                    .flex()
                    .relative()
                    .overflow_hidden()
                    .child(self.comment_gutter(file_path, row.new_number, px(SPLIT_LEFT_PAD), cx))
                    .child(
                        div()
                            .w(line_number_width)
                            .flex_none()
                            .text_right()
                            .text_color(rgb(if selected { 0xffffff } else { 0x70798a }))
                            .child(row.new_number.map(|n| n.to_string()).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .w(px(SPLIT_RIGHT_PAD))
                            .flex_none()
                            .flex()
                            .items_center()
                            .child(if let Some(section) = right_fold {
                                Self::expanded_fold_icon(section, px(SPLIT_RIGHT_PAD), cx).into_any_element()
                            } else {
                                div().into_any_element()
                            }),
                    )
                    .child(
                        restrict_wheel_scroll_to_matching_axis(
                            div()
                                .id(("new-code", index))
                                .flex_1()
                                .when(!right_gap_marker, |element| element.bg(rgb(right_code_bg)))
                                .overflow_x_scroll()
                                .track_scroll(&right_code_scroll),
                        )
                            .child(
                                div()
                                    .min_w(right_content_width)
                                    .whitespace_nowrap()
                                    .text_color(rgb(TEXT))
                                    .child(Self::syntax_text_with_emphasis(
                                        row.new_text.clone(), &new_syntax,
                                        row.new_segments.as_deref(), 0x285b4b,
                                    )),
                            ),
                    ),
            )
    }

    fn expanded_fold_icon(section: UnchangedSection, width: Pixels, cx: &mut Context<Self>) -> impl IntoElement {
        let id = format!("fold-{}-{}", section.file_path, section.start_row);
        let icon_size = if width < px(12.) { width } else { px(12.) };
        div().id(gpui::SharedString::from(id)).w(width).h_full().flex().items_center().justify_center()
            .cursor_pointer()
            .tooltip(|_, cx| cx.new(|_| ReviewTooltip("Collapse unchanged declaration".into())).into())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.toggle_unchanged_section(section.clone(), cx);
            }))
            .child(svg().path("icons/chevron_down.svg").size(icon_size).text_color(rgb(MUTED)))
    }

    fn unified_diff_line(
        &self,
        file_path: &str,
        index: usize,
        variant: &str,
        fold: Option<UnchangedSection>,
        old_number: Option<usize>,
        new_number: Option<usize>,
        background: u32,
        text: String,
        syntax: Vec<crate::command::diff::highlight::SyntaxRange>,
        segments: Option<Vec<crate::command::diff::types::InlineSegment>>,
        is_deleted: bool,
        line_number_width: Pixels,
        content_width: Pixels,
        code_scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let comment_line = if is_deleted { None } else { new_number };
        let select_path = file_path.to_string();
        let drag_path = file_path.to_string();
        let menu_path = file_path.to_string();
        let selected = self.line_is_selected(file_path, comment_line);
        let highlighted = self.comment_highlight(file_path, comment_line);
        let code_background = if selected {
            if is_deleted {
                0x65404a
            } else if variant == "added" {
                0x355747
            } else {
                0x3b4d68
            }
        } else if highlighted {
            super::comments_view::COMMENT_BG
        } else {
            background
        };
        div()
            .id(gpui::SharedString::from(format!("unified-row-{index}-{variant}")))
            .h(px(22.))
            .w_full()
            .flex()
            .relative()
            .overflow_hidden()
            .bg(rgb(BG))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, event, _, cx| this.select_comment_line(&select_path, comment_line, event, cx)))
            .on_mouse_move(cx.listener(move |this, event, _, cx| this.extend_comment_selection(&drag_path, comment_line, event, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, event, window, cx| this.show_comment_context_menu(&menu_path, comment_line, event, window, cx)))
            .font_family("JetBrains Mono")
            .text_size(px(12.))
            .child(self.comment_gutter(file_path, comment_line, px(UNIFIED_LEFT_PAD), cx))
            .child(
                div()
                    .w(line_number_width)
                    .flex_none()
                    .text_right()
                    .text_color(rgb(0x70798a))
                    .child(old_number.map(|line| line.to_string()).unwrap_or_default()),
            )
            .child(
                div()
                    .w(px(UNIFIED_COUNTER_GAP))
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(if let Some(section) = fold {
                        Self::expanded_fold_icon(section, px(UNIFIED_COUNTER_GAP), cx).into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
            .child(
                div()
                    .w(line_number_width)
                    .flex_none()
                    .text_left()
                    .text_color(rgb(if selected { 0xffffff } else { 0x70798a }))
                    .child(new_number.map(|line| line.to_string()).unwrap_or_default()),
            )
            .child(div().w(px(UNIFIED_RIGHT_PAD)).flex_none())
            .child(
                restrict_wheel_scroll_to_matching_axis(
                    div()
                        .id(gpui::SharedString::from(format!("unified-code-{index}-{variant}")))
                        .flex_1()
                        .relative()
                        .bg(rgb(code_background))
                        .overflow_x_scroll()
                        .track_scroll(&code_scroll),
                )
                    .child(
                        div()
                            .min_w(content_width)
                            .relative()
                            .whitespace_nowrap()
                            .text_color(rgb(TEXT))
                            .child(Self::syntax_text_with_emphasis(
                                text, &syntax, segments.as_deref(),
                                if is_deleted { 0x713641 } else { 0x285b4b },
                            )),
                    ),
            )
            .into_any_element()
    }

    fn collapsed_unchanged_unified_row(
        section: UnchangedSection,
        line_number_width: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let count = section.end_row - section.start_row;
        let label = Self::fold_label(&section, count);
        let row_id = format!(
            "collapsed-unchanged-unified-{}-{}",
            section.file_path, section.start_row
        );
        div()
            .id(gpui::SharedString::from(row_id))
            .w_full()
            .h(px(22.))
            .flex()
            .items_center()
            .cursor_pointer()
            .bg(rgb(BG))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_unchanged_section(section.clone(), cx)
            }))
            // Keep the fold label aligned with the unified code column.
            .child(div().w(px(UNIFIED_LEFT_PAD + UNIFIED_COUNTER_GAP + UNIFIED_RIGHT_PAD) + line_number_width * 2.).flex_none())
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .hover(|element| element.bg(rgb(PANEL)))
                    .child(Self::collapsed_unchanged_pane(px(0.), label, true)),
            )
    }

    fn collapsed_unchanged_row(
        section: UnchangedSection,
        left_width: Pixels,
        line_number_width: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let count = section.end_row - section.start_row;
        let label = Self::fold_label(&section, count);
        let row_id = format!(
            "collapsed-unchanged-{}-{}",
            section.file_path, section.start_row
        );
        div()
            .id(gpui::SharedString::from(row_id))
            .w_full()
            // Keep this equal to `diff_row`: these rows share one virtualized list.
            .h(px(22.))
            .flex()
            .items_center()
            .cursor_pointer()
            .bg(rgb(BG))
            .on_click(
                cx.listener(move |this, _, _, cx| {
                    this.toggle_unchanged_section(section.clone(), cx)
                }),
            )
            .child(Self::collapsed_unchanged_split_pane(
                left_width, px(SPLIT_LEFT_PAD + SPLIT_RIGHT_PAD) + line_number_width, label.clone(), false,
            ))
            .child(div().w(px(2.)).h_full().bg(rgb(BORDER)))
            .child(Self::collapsed_unchanged_split_pane(
                px(0.),
                px(SPLIT_LEFT_PAD + SPLIT_RIGHT_PAD) + line_number_width,
                label,
                true,
            ))
    }

    fn collapsed_unchanged_split_pane(
        width: Pixels,
        gutter_width: Pixels,
        label: String,
        fill: bool,
    ) -> impl IntoElement {
        div()
            .when(width > px(0.), |element| element.w(width))
            .when(fill, |element| element.flex_1())
            .min_w(px(0.))
            .h_full()
            .flex()
            .child(div().w(gutter_width).flex_none())
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .hover(|element| element.bg(rgb(PANEL)))
                    .child(Self::collapsed_unchanged_pane(px(0.), label, true)),
            )
    }

    fn fold_label(section: &UnchangedSection, count: usize) -> String {
        let mut label = format!("{count} unchanged {}", if count == 1 { "line" } else { "lines" });
        if let Some(symbol) = &section.label {
            label.push_str("   │   ");
            label.push_str(symbol);
        }
        label
    }

    fn collapsed_unchanged_pane(width: Pixels, label: String, fill: bool) -> impl IntoElement {
        div()
            .when(width > px(0.), |element| element.w(width))
            .when(fill, |element| element.flex_1())
            .min_w(px(0.))
            .h_full()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(11.))
            .text_color(rgb(0x707b8c))
            .child(svg().path("icons/chevron_down.svg").size(px(12.)).text_color(rgb(MUTED)))
            .child(div().whitespace_nowrap().child(label))
            .child(div().h(px(1.)).flex_1().bg(rgb(0x3b4351)))
    }

    /// One continuous hatch behind both code panes. Individual empty rows leave
    /// their background transparent so their stripe phase does not restart at
    /// every virtualized row boundary.
    fn diff_gap_hatches(left_width: Pixels) -> impl IntoElement {
        div()
            .id("diff-gap-hatches")
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .right_0()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(left_width)
                    .bg(pattern_slash(rgb(0x3a4351).into(), 2., 14.)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(left_width + px(2.))
                    .right_0()
                    .bg(pattern_slash(rgb(0x3a4351).into(), 2., 14.)),
            )
    }

    pub(super) fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.tabs.iter().enumerate().filter_map(|(tab_index, tab)| {
            let file = self.model.files.get(tab.file_index)?;
            let hover_group = format!("review-tab-hover-{tab_index}");
            let label = file
                .path
                .rsplit('/')
                .next()
                .unwrap_or(file.path.as_str())
                .to_string();
            let display_label = middle_ellipsize(&label, 24);
            let active = self.is_file_selected(tab.file_index);
            let is_preview = tab.is_preview;
            Some(
                div()
                    .id(("review-tab", tab_index))
                    .h_full()
                    .max_w(px(240.))
                    // Mirror the close button's gutter on the left so the label
                    // remains centered whether or not the button is visible.
                    .pl(px(28.))
                    .pr(px(28.))
                    .flex()
                    .relative()
                    .items_center()
                    .cursor_pointer()
                    .group(hover_group.clone())
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    // The shared tab-strip divider is redrawn on inactive
                    // tabs only. The active tab stays connected to its
                    // breadcrumb below.
                    .when(!active, |element| element.border_b_1())
                    .bg(if active { rgb(BG) } else { rgb(PANEL) })
                    .text_size(px(11.))
                    .text_color(if active { rgb(TEXT) } else { rgb(MUTED) })
                    .when(is_preview, |element| element.italic())
                    .hover(|element| element.bg(rgb(0x2c3443)))
                    .tooltip(move |_, cx| cx.new(|_| ReviewTooltip(label.clone())).into())
                    .on_click(cx.listener(move |this, _, _, cx| this.activate_tab(tab_index, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_center()
                            .child(display_label),
                    )
                    .child(
                        div()
                            .id(("close-review-tab", tab_index))
                            .absolute()
                            .top(px(7.))
                            .right(px(4.))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .invisible()
                            .group_hover(hover_group, |element| element.visible())
                            .hover(|element| element.rounded_sm().bg(rgb(0x465166)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_tab(tab_index, cx);
                            }))
                            .child("×"),
                    ),
            )
        });
        let log_tabs = self
            .log_tabs
            .iter()
            .copied()
            .enumerate()
            .map(|(log_index, log)| {
                let active = self.active_log == Some(log);
                let hover_group = format!("log-tab-hover-{}", log.title());
                div()
                    .id(("log-tab", log_index))
                    .h_full()
                    .max_w(px(240.))
                    .pl(px(10.))
                    .pr(px(28.))
                    .flex()
                    .relative()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .group(hover_group.clone())
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    .when(!active, |element| element.border_b_1())
                    .bg(if active { rgb(BG) } else { rgb(PANEL) })
                    .text_size(px(11.))
                    .text_color(if active { rgb(TEXT) } else { rgb(MUTED) })
                    .hover(|element| element.bg(rgb(0x2c3443)))
                    .on_click(cx.listener(move |this, _, _, cx| this.activate_log_tab(log, cx)))
                    .child(
                        svg()
                            .path("icons/log.svg")
                            .size(px(14.))
                            .text_color(rgb(if active { BLUE } else { MUTED })),
                    )
                    .child(div().flex_1().truncate().text_center().child(log.title()))
                    .child(
                        div()
                            .id(("close-log-tab", log_index))
                            .absolute()
                            .top(px(7.))
                            .right(px(4.))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .invisible()
                            .group_hover(hover_group, |element| element.visible())
                            .hover(|element| element.rounded_sm().bg(rgb(0x465166)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_log_tab(log, cx);
                            }))
                            .child("×"),
                    )
            });
        div()
            .id("review-tabs")
            .h(px(32.))
            .w_full()
            .relative()
            // The tabs and comparison breadcrumb deliberately share one
            // uninterrupted surface.
            .bg(rgb(BG))
            // Preserve the tab-strip divider everywhere except under the
            // active tab, which renders above this line.
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(px(1.))
                    .bg(rgb(BORDER)),
            )
            .child(
                div()
                    .id("review-tabs-scroll")
                    .h_full()
                    .w_full()
                    .flex()
                    .overflow_scroll()
                    .relative()
                    .children(tabs)
                    .children(log_tabs),
            )
    }

    fn log_canvas(&self, log: LogTab, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = match log {
            LogTab::Lsp => &self.lsp_logs,
        };
        let entries_count = entries.len();
        let rows = entries.iter().enumerate().map(|(index, entry)| {
            div()
                .id(("log-entry", index))
                .min_h(px(26.))
                .px_3()
                .py_1()
                .flex()
                .items_start()
                .gap_2()
                .border_b_1()
                .border_color(rgb(BORDER))
                .text_size(px(11.))
                .child(
                    div()
                        .w(px(28.))
                        .flex_none()
                        .text_color(rgb(MUTED))
                        .child(format!("{}", index + 1)),
                )
                .child(div().text_color(rgb(TEXT)).child(entry.clone()))
        });
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .child(self.tab_bar(cx))
            .child(
                div()
                    .h(px(44.))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "{entries_count} {}",
                                if entries_count == 1 { "entry" } else { "entries" }
                            )),
                    )
                    .child(
                        div()
                            .id("clear-log-entries")
                            .size(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .text_color(rgb(if entries.is_empty() { 0x697383 } else { TEXT }))
                            .when(!entries.is_empty(), |button| {
                                button
                                    .cursor_pointer()
                                    .hover(|button| button.bg(rgb(0x3a4350)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.clear_log_entries(log, cx)
                                    }))
                            })
                            .tooltip(|_, cx| {
                                cx.new(|_| ReviewTooltip("Clear log entries".into())).into()
                            })
                            .child(svg().path("icons/trash.svg").size(px(14.))),
                    ),
            )
            .child(
                div()
                    .id("log-scroll")
                    .flex_1()
                    .overflow_scroll()
                    .when(entries.is_empty(), |element| {
                        element.child(
                            div()
                                .p_4()
                                .text_size(px(11.))
                                .text_color(rgb(MUTED))
                                .child("No log entries yet."),
                        )
                    })
                    .children(rows),
            )
    }

    fn diff_panel_titles(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.diff_view == DiffView::Unified {
            return div()
                .id("diff-panel-titles")
                .h(px(28.))
                .w_full()
                .flex()
                .border_b_1()
                .border_color(rgb(BORDER))
                .bg(rgb(BG))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .flex()
                        .items_center()
                        .px_3()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(11.))
                        .text_color(rgb(TEXT))
                        .child(format!("HEAD ({}) → Working Copy", self.commit_id)),
                )
                .child(self.diff_change_navigation(cx));
        }

        div()
            .id("diff-panel-titles")
            .h(px(28.))
            .w_full()
            .flex()
            .border_b_1()
            .border_color(rgb(BORDER))
            .bg(rgb(BG))
            .shadow_sm()
            .child(
                div()
                    .w(self.diff_left_width)
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_3()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(11.))
                    .text_color(rgb(TEXT))
                    .child(format!("HEAD ({})", self.commit_id)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .flex()
                            .items_center()
                            .px_3()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(11.))
                            .text_color(rgb(TEXT))
                            .child("Working Copy"),
                    )
                    .child(self.diff_change_navigation(cx)),
            )
    }

    fn scroll_thumb(&self, handle: &ScrollHandle, width: Pixels) -> (Pixels, Pixels) {
        let track_width = (width - px(16.)).max(px(1.));
        let max_offset = handle.max_offset().width;
        if max_offset <= px(0.) {
            return (px(0.), track_width);
        }
        let viewport = handle.bounds().size.width.max(px(1.));
        let thumb_width = (track_width * (viewport / (viewport + max_offset)))
            .clamp(px(32.).min(track_width), track_width);
        let progress = (-handle.offset().x / max_offset).clamp(0., 1.);
        (progress * (track_width - thumb_width), thumb_width)
    }

    fn horizontal_scroll_indicator(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let left_width = self.diff_left_width;
        let right_width = self.right_code_scroll.bounds().size.width + px(66.);
        let (left_thumb_offset, left_thumb_width) =
            self.scroll_thumb(&self.left_code_scroll, left_width);
        let (right_thumb_offset, right_thumb_width) =
            self.scroll_thumb(&self.right_code_scroll, right_width);
        div()
            .h(px(12.))
            .w_full()
            .flex()
            .items_center()
            .bg(rgb(BG))
            .child(
                div()
                    .id("left-horizontal-scrollbar")
                    .h(px(10.))
                    .w(left_width)
                    .px_2()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::start_left_code_scroll))
                    .child(
                        div()
                            .h(px(2.))
                            .w_full()
                            .rounded_full()
                            .bg(rgb(0x303847))
                            .child(
                                div()
                                    .h_full()
                                    .w(left_thumb_width)
                                    .ml(left_thumb_offset)
                                    .rounded_full()
                                    .bg(rgb(0x6f7b8f)),
                            ),
                    ),
            )
            .child(div().w(px(2.)).h_full().bg(rgb(BORDER)))
            .child(
                div()
                    .id("right-horizontal-scrollbar")
                    .h(px(10.))
                    .flex_1()
                    .px_2()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::start_right_code_scroll),
                    )
                    .child(
                        div()
                            .h(px(2.))
                            .w_full()
                            .rounded_full()
                            .bg(rgb(0x303847))
                            .child(
                                div()
                                    .h_full()
                                    .w(right_thumb_width)
                                    .ml(right_thumb_offset)
                                    .rounded_full()
                                    .bg(rgb(0x6f7b8f)),
                            ),
                    ),
            )
    }

    fn diff_change_navigation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let unchanged_sections = self
            .selected_file()
            .map(|file| logical_unchanged_sections(file, self.lsp_symbols.get(&file.path).map(Vec::as_slice).unwrap_or(&[])))
            .unwrap_or_default();
        let has_unchanged_sections = !unchanged_sections.is_empty();
        let all_unchanged_sections_collapsed = has_unchanged_sections
            && unchanged_sections
                .iter()
                .all(|section| self.collapsed_unchanged_sections.contains(section));
        let total_changes = self
            .selected_file()
            .map(|file| change_start_rows(&file.rows).len())
            .unwrap_or(0);
        let current_change = (total_changes > 0)
            .then_some(self.selected_change.saturating_add(1).min(total_changes))
            .unwrap_or(0);
        let can_select_previous = current_change > 1;
        let can_select_next = current_change < total_changes;
        let can_toggle_diff_view = self.selected_file().is_some();
        let target_diff_view = match self.diff_view {
            DiffView::Split => DiffView::Unified,
            DiffView::Unified => DiffView::Split,
        };
        let diff_view_tooltip = match target_diff_view {
            DiffView::Split => "Switch to split view",
            DiffView::Unified => "Switch to unified view",
        };
        let change_button = |id, up, enabled| {
            div()
                .id(id)
                .w(px(22.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(if enabled { rgb(TEXT) } else { rgb(MUTED) })
                .when(enabled, |element| {
                    element
                        .cursor_pointer()
                        .hover(|element| element.bg(rgb(0x35425a)))
                })
                .child(Self::change_arrow_icon(up, enabled))
        };

        div()
            .id("diff-change-navigation")
            .h_full()
            .px_2()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .whitespace_nowrap()
            .text_size(px(11.))
            .text_color(rgb(MUTED))
            .child(format!("Change {current_change} of {total_changes}"))
            .child(
                change_button("previous-file-change", true, can_select_previous).when(
                    can_select_previous,
                    |element| {
                        element.on_click(
                            cx.listener(|this, _, _, cx| this.select_previous_file_change(cx)),
                        )
                    },
                ),
            )
            .child(
                change_button("next-file-change", false, can_select_next).when(
                    can_select_next,
                    |element| {
                        element.on_click(
                            cx.listener(|this, _, _, cx| this.select_next_file_change(cx)),
                        )
                    },
                ),
            )
            .child(div().w(px(1.)).h(px(16.)).mx_1().bg(rgb(BORDER)))
            .child(
                div()
                    .id("toggle-diff-view")
                    .w(px(22.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(if can_toggle_diff_view { rgb(TEXT) } else { rgb(MUTED) })
                    .when(can_toggle_diff_view, |element| {
                        element
                            .cursor_pointer()
                            .hover(|element| element.bg(rgb(0x35425a)))
                            .tooltip({
                                let tooltip = diff_view_tooltip.to_string();
                                move |_, cx| {
                                    cx.new(|_| ReviewTooltip(tooltip.clone())).into()
                                }
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_diff_view(cx)))
                    })
                    .child(Self::diff_view_icon(target_diff_view)),
            )
            .child(div().w(px(1.)).h(px(16.)).mx_1().bg(rgb(BORDER)))
            .child(
                div()
                    .id("toggle-unchanged-sections")
                    .w(px(22.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(if has_unchanged_sections {
                        rgb(if self.hide_unchanged_sections {
                            TEXT
                        } else {
                            MUTED
                        })
                    } else {
                        rgb(0x5d6675)
                    })
                    .when(self.hide_unchanged_sections, |element| {
                        element.bg(rgb(0x35425a))
                    })
                    .when(has_unchanged_sections, |element| {
                        element
                            .cursor_pointer()
                            .hover(|element| element.bg(rgb(0x35425a)))
                            .tooltip(move |_, cx| cx.new(|_| ReviewTooltip(
                                if all_unchanged_sections_collapsed { "Expand all unchanged sections" } else { "Collapse all unchanged sections" }.into()
                            )).into())
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.toggle_all_unchanged_sections(cx)
                                }),
                            )
                    })
                    // The same control toggles between collapsing all context
                    // and restoring it; the inline dividers remain available
                    // for expanding one section at a time.
                    .child(Self::unchanged_context_icon(
                        all_unchanged_sections_collapsed,
                    )),
            )
    }

    pub(super) fn diff_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(log) = self.active_log {
            return self.log_canvas(log, cx).into_any_element();
        }
        if self.radar.active {
            return div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .flex_col()
                .child(self.radar_canvas(cx))
                .into_any_element();
        }
        let Some(file) = self.selected_file() else {
            let message = if self.active_file_load.is_some() { "Loading file…" } else { "No changed files" };
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(self.tab_bar(cx))
                // Keep the empty state horizontally centered and one-third
                // down the available diff canvas rather than at its top edge.
                .child(div().flex_1())
                .child(
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .text_color(rgb(MUTED))
                        .child(message),
                )
                .child(div().flex_grow())
                .into_any_element();
        };
        let source_rows = file.rows.clone();
        let semantic = file.semantic.clone();
        let symbols = self.lsp_symbols.get(&file.path).map(Vec::as_slice).unwrap_or(&[]);
        let sections = logical_unchanged_sections(file, symbols);
        let fold_starts = sections.iter()
            .map(|section| (section.start_row, section.clone())).collect::<HashMap<_, _>>();
        let display_rows = display_rows_from_sections(file.rows.len(), &sections, &self.collapsed_unchanged_sections);
        let selected = self.model.selected;
        // The shared handles need every virtualized row to have the same minimum
        // content width; otherwise whichever short row is painted last clamps the
        // handle back to zero and horizontal scrolling appears broken.
        let left_content_width = px((file.max_old_chars as f32 * 8.).max(1.));
        let right_content_width = px((file.max_new_chars as f32 * 8.).max(1.));
        let unified_content_width = left_content_width.max(right_content_width);
        let line_number_digits = file.line_number_digits;
        let split_line_number_width = px((line_number_digits as f32 * 8. + 2.).max(24.));
        let unified_line_number_width = px((line_number_digits as f32 * 7.5).max(24.));
        let rows = if source_rows.is_empty() {
            div()
                .p_6()
                .text_color(rgb(MUTED))
                .child("Binary file — contents are intentionally not rendered.")
                .into_any_element()
        } else if self.diff_view == DiffView::Split {
            let left_width = self.diff_left_width;
            let left_code_scroll = self.left_code_scroll.clone();
            let right_code_scroll = self.right_code_scroll.clone();
            let diff_list_scroll = self.diff_list_scroll.clone();
            let file_path = file.path.clone();
            uniform_list(
                gpui::SharedString::from(format!("diff-rows-{selected}-{}", self.model_revision)),
                display_rows.len(),
                cx.processor(move |this, range: Range<usize>, _, cx| {
                    range
                        .map(|index| match &display_rows[index] {
                            DiffDisplayRow::Code { source_row } => this.diff_row(
                                &file_path,
                                &source_rows[*source_row],
                                *source_row,
                                fold_starts.get(source_row).cloned(),
                                source_rows[*source_row]
                                    .old_number
                                    .and_then(|line| semantic.old_syntax.get(&line).cloned())
                                    .unwrap_or_default(),
                                source_rows[*source_row]
                                    .new_number
                                    .and_then(|line| semantic.new_syntax.get(&line).cloned())
                                    .unwrap_or_default(),
                                split_line_number_width,
                                left_width,
                                left_content_width,
                                right_content_width,
                                left_code_scroll.clone(),
                                right_code_scroll.clone(),
                                cx,
                            )
                            .into_any_element(),
                            DiffDisplayRow::CollapsedUnchanged { section } => {
                                Self::collapsed_unchanged_row(section.clone(), left_width, split_line_number_width, cx)
                                    .into_any_element()
                            }
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(diff_list_scroll)
            .size_full()
            .into_any_element()
        } else {
            let mut unified_rows = Vec::with_capacity(display_rows.len());
            for row in display_rows {
                match row {
                    DiffDisplayRow::Code { source_row } => {
                        let change = source_rows[source_row].change;
                        unified_rows.push(UnifiedDisplayRow::Code {
                            source_row,
                            old_side: matches!(
                                change,
                                crate::command::diff::types::ChangeType::Delete
                                    | crate::command::diff::types::ChangeType::Modified
                            ),
                        });
                        if matches!(change, crate::command::diff::types::ChangeType::Modified) {
                            unified_rows.push(UnifiedDisplayRow::Code {
                                source_row,
                                old_side: false,
                            });
                        }
                    }
                    DiffDisplayRow::CollapsedUnchanged { section } => {
                        unified_rows.push(UnifiedDisplayRow::Collapsed(section))
                    }
                }
            }
            let file_path = file.path.clone();
            let code_scroll = self.right_code_scroll.clone();
            let list_scroll = self.unified_diff_scroll.clone();
            uniform_list(
                gpui::SharedString::from(format!(
                    "unified-rows-{selected}-{}",
                    self.model_revision
                )),
                unified_rows.len(),
                cx.processor(move |this, range: Range<usize>, _, cx| {
                    range
                        .map(|index| match &unified_rows[index] {
                            UnifiedDisplayRow::Collapsed(section) => {
                                Self::collapsed_unchanged_unified_row(
                                    section.clone(),
                                    unified_line_number_width,
                                    cx,
                                )
                                .into_any_element()
                            }
                            UnifiedDisplayRow::Code {
                                source_row,
                                old_side,
                            } => {
                                let row = &source_rows[*source_row];
                                let (variant, number, background, text, syntax, segments) =
                                    if *old_side {
                                        (
                                            "removed",
                                            row.old_number,
                                            0x35292f,
                                            row.old_text.clone(),
                                            row.old_number
                                                .and_then(|line| {
                                                    semantic.old_syntax.get(&line).cloned()
                                                })
                                                .unwrap_or_default(),
                                            row.old_segments.clone(),
                                        )
                                    } else {
                                        let changed = !matches!(
                                            row.change,
                                            crate::command::diff::types::ChangeType::Equal
                                        );
                                        (
                                            if changed { "added" } else { "context" },
                                            row.new_number,
                                            if changed { 0x253a32 } else { BG },
                                            row.new_text.clone(),
                                            row.new_number
                                                .and_then(|line| {
                                                    semantic.new_syntax.get(&line).cloned()
                                                })
                                                .unwrap_or_default(),
                                            row.new_segments.clone(),
                                        )
                                    };
                                this.unified_diff_line(
                                    &file_path,
                                    index,
                                    variant,
                                    if *old_side {
                                        None
                                    } else {
                                        fold_starts.get(source_row).cloned()
                                    },
                                    if *old_side { number } else { None },
                                    if *old_side { None } else { number },
                                    background,
                                    text,
                                    syntax,
                                    segments,
                                    *old_side,
                                    unified_line_number_width,
                                    unified_content_width,
                                    code_scroll.clone(),
                                    cx,
                                )
                            }
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(list_scroll)
            .size_full()
            .into_any_element()
        };
        div()
            .flex()
            .min_w(px(0.))
            .flex_col()
            .size_full()
            .relative()
            .bg(rgb(BG))
            .child(self.tab_bar(cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .relative()
                    .overflow_hidden()
                    .child(self.diff_panel_titles(cx))
                    .child(
                        div()
                            .id("diff-scroll")
                            .flex_1()
                            .relative()
                            .overflow_scroll()
                            .when(self.diff_view == DiffView::Split, |element| {
                                element.child(Self::diff_gap_hatches(self.diff_left_width))
                            })
                            .child(rows),
                    )
                    .when(self.diff_view == DiffView::Split, |element| element
                        .child(div()
                            .absolute()
                            .top(px(28.))
                            .bottom_0()
                            .left(self.diff_left_width - px(3.))
                            .w(px(3.))
                            .bg(gpui::rgba(0x00000022)))
                        .child(div()
                            .absolute()
                            .top(px(28.))
                            .bottom_0()
                            .left(self.diff_left_width - px(1.))
                            .w(px(1.))
                            .bg(gpui::rgba(0x00000030)))
                        .child(
                        div()
                            .id("diff-resizer")
                            .absolute()
                            // The panel divider starts below the unified
                            // breadcrumb, so it separates code panels without
                            // splitting the header into two sections.
                            .top(px(28.))
                            .bottom_0()
                            .left(self.diff_left_width)
                            .w(px(2.))
                            .cursor(CursorStyle::ResizeLeftRight)
                            .bg(rgb(BORDER))
                            .hover(|element| element.bg(rgb(BLUE)))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::start_diff_drag))
                            .on_mouse_move(cx.listener(Self::move_diff_drag))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::stop_drag)),
                    )),
            )
            .when(self.diff_view == DiffView::Split, |element| {
                element.child(self.horizontal_scroll_indicator(cx))
            })
            .into_any_element()
    }

    pub(super) fn annotations(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active_path = self
            .selected_file()
            .map(|file| file.path.clone())
            .unwrap_or_default();
        let notes = self
            .model
            .annotations
            .iter()
            .filter(|note| note.file == active_path)
            .map(|note| {
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x2c3443))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_sm()
                    .text_color(rgb(TEXT))
                    .child(
                        div()
                            .mb_1()
                            .text_size(px(11.))
                            .text_color(rgb(BLUE))
                            .child(format!("{}:{}", note.file, note.line)),
                    )
                    .child(note.body.clone())
            });
        div()
            .w(self.annotation_width)
            .h_full()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(PANEL))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_sm().text_color(rgb(TEXT)).child("☷"))
                    .child(
                        div()
                            .id("add-note")
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .bg(rgb(0x35548a))
                            .text_size(px(11.))
                            .text_color(rgb(TEXT))
                            .hover(|element| element.bg(rgb(0x4267a7)))
                            .on_click(cx.listener(|this, _, _, cx| this.add_annotation(cx)))
                            .child("+"),
                    ),
            )
            .child(
                div()
                    .id("annotation-scroll")
                    .flex_1()
                    .overflow_scroll()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(notes),
            )
            .child(
                div().text_size(px(11.)).text_color(rgb(MUTED)).child(
                    "Notes are local review metadata; source files are never editable here.",
                ),
            )
    }
}
