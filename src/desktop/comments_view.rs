//! Working-copy comment gutter, composer, and review collection.
use super::comments::{follow_up_prompt, Comment, CommentTarget, LineSelection};
use super::view::ReviewTooltip;
use super::*;
use gpui::{Animation, AnimationExt};
use gpui::{AnyElement, Div, Stateful};

pub(super) const COMMENT_BG: u32 = 0x443d2c;

impl ReviewWorkspace {
    fn current_comments(&self) -> &[Comment] {
        self.review_comments
            .repositories
            .get(&self.repository_root)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(super) fn can_comment(&self) -> bool {
        matches!(
            self.reference,
            None | Some(CommitReference::RangeToWorkingTree { .. })
        )
    }

    pub(super) fn comment_highlight(&self, file: &str, line: Option<usize>) -> bool {
        self.can_comment()
            && line.is_some_and(|line| {
                self.current_comments()
                    .iter()
                    .any(|note| note.target.contains(file, line))
                    || self
                        .review_comments
                        .draft
                        .as_ref()
                        .is_some_and(|range| range.contains(file, line))
            })
    }

    pub(super) fn line_is_selected(&self, file: &str, line: Option<usize>) -> bool {
        self.can_comment()
            && self.review_comments.selection.file == file
            && line.is_some_and(|line| self.review_comments.selection.lines.contains(&line))
    }

    pub(super) fn comment_gutter(
        &self,
        file: &str,
        line: Option<usize>,
        width: Pixels,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let notes = self
            .current_comments()
            .iter()
            .filter(|note| {
                self.can_comment() && line.is_some_and(|line| note.target.contains(file, line))
            })
            .collect::<Vec<_>>();
        let tip = notes
            .iter()
            .map(|note| note.body.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        div()
            .id(("comment-gutter", line.unwrap_or_default()))
            .w(width)
            .h(px(22.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .when(!notes.is_empty(), |gutter| {
                gutter
                    .child(if width < px(7.) {
                        div().w(width).h(px(12.)).bg(rgb(YELLOW)).into_any_element()
                    } else {
                        svg()
                            .path("icons/comment.svg")
                            .size(px(14.))
                            .text_color(rgb(YELLOW))
                            .into_any_element()
                    })
                    .tooltip(move |_, cx| cx.new(|_| ReviewTooltip(tip.clone())).into())
            })
            .into_any_element()
    }

    pub(super) fn select_comment_line(
        &mut self,
        file: &str,
        line: Option<usize>,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(line) = line.filter(|_| self.can_comment()) else {
            return;
        };
        cx.stop_propagation();
        self.review_comments.selection.click(
            file,
            line,
            event.modifiers.secondary(),
            event.modifiers.shift,
        );
        cx.notify();
    }

    pub(super) fn extend_comment_selection(
        &mut self,
        file: &str,
        line: Option<usize>,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if event.dragging()
            && line.is_some_and(|line| self.review_comments.selection.drag_to(file, line))
        {
            cx.notify();
        }
    }

    pub(super) fn finish_comment_selection(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.review_comments.selection.dragging = false;
    }

    pub(super) fn show_comment_context_menu(
        &mut self,
        file: &str,
        line: Option<usize>,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(line) = line.filter(|_| self.can_comment()) else {
            return;
        };
        cx.stop_propagation();
        if !self.line_is_selected(file, Some(line)) {
            self.review_comments
                .selection
                .click(file, line, false, false);
        }
        self.review_comments.selection.dragging = false;
        self.close_menus();
        self.review_comments.context_menu = Some((
            f32::from(
                event
                    .position
                    .x
                    .min(window.viewport_size().width - px(190.))
                    .max(px(0.)),
            ),
            f32::from(
                event
                    .position
                    .y
                    .min(window.viewport_size().height - px(42.))
                    .max(px(0.)),
            ),
        ));
        cx.notify();
    }

    fn compose_selected_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.review_comments.draft = self.review_comments.selection.target();
        self.close_menus();
        self.comment_input.update(cx, |input, cx| input.clear(cx));
        let focus = self.comment_input.read(cx).focus_handle().clone();
        window.focus(&focus);
        cx.notify();
    }

    pub(super) fn comment_context_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (x, y) = self.review_comments.context_menu.unwrap();
        div()
            .id("comment-context-menu")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(184.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(0x303641))
            .shadow_lg()
            .child(
                Self::comment_action(
                    "add-selected-comment",
                    "icons/comment.svg",
                    "Add Comment",
                    TEXT,
                    true,
                )
                .on_click(
                    cx.listener(|this, _, window, cx| this.compose_selected_comment(window, cx)),
                ),
            )
    }

    pub(super) fn cancel_review_comment(&mut self, cx: &mut Context<Self>) {
        self.review_comments.draft = None;
        self.review_comments.selection = LineSelection::default();
        self.comment_input.update(cx, |input, cx| input.clear(cx));
        cx.notify();
    }

    pub(super) fn save_review_comment(&mut self, body: &str, cx: &mut Context<Self>) {
        if self.review_comments.add(self.repository_root.clone(), body) {
            self.comment_input.update(cx, |input, cx| input.clear(cx));
            cx.notify();
        }
    }

    pub(super) fn open_comments(&mut self, cx: &mut Context<Self>) {
        self.close_menus();
        self.review_comments.active = true;
        cx.notify();
    }

    fn close_comments(&mut self, cx: &mut Context<Self>) {
        self.review_comments.active = false;
        cx.notify();
    }

    fn clear_review_comments(&mut self, cx: &mut Context<Self>) {
        self.review_comments
            .repositories
            .remove(&self.repository_root);
        self.review_comments.copied = false;
        self.review_comments.copy_epoch = self.review_comments.copy_epoch.wrapping_add(1);
        self.close_menus();
        self.cancel_review_comment(cx);
    }

    fn copy_review_comments(&mut self, cx: &mut Context<Self>) {
        let prompt = follow_up_prompt(self.current_comments());
        if prompt.is_empty() {
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(prompt));
        self.review_comments.copied = true;
        self.review_comments.copy_epoch = self.review_comments.copy_epoch.wrapping_add(1);
        let epoch = self.review_comments.copy_epoch;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            if let Some(this) = this.upgrade() {
                let _ = this.update(cx, |this, cx| {
                    if epoch == this.review_comments.copy_epoch {
                        this.review_comments.copied = false;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn show_comment_source(&mut self, target: &CommentTarget, cx: &mut Context<Self>) {
        self.choose_sidebar_file(target.file.clone(), true, cx);
        // Expand the entire target, including unchanged context, before scrolling.
        let Some(file) = self.selected_file() else {
            return;
        };
        let rows = file.rows.clone();
        self.collapsed_unchanged_sections.retain(|section| {
            section.file_path != target.file
                || !rows
                    .get(section.start_row..section.end_row)
                    .unwrap_or_default()
                    .iter()
                    .any(|row| {
                        row.new_number
                            .is_some_and(|line| target.contains(&target.file, line))
                    })
        });
        if let Some(file) = self.selected_file() {
            let display_rows = display_rows_for_file(
                file,
                &self.collapsed_unchanged_sections,
                self.lsp_symbols
                    .get(&file.path)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            );
            if let Some(row) = display_rows.iter().position(|row| {
                matches!(row, DiffDisplayRow::Code { source_row } if file.rows[*source_row].new_number == target.ranges.first().map(|range| range.start))
            }) {
                if self.diff_view == DiffView::Split {
                    self.diff_list_scroll.scroll_to_item_strict(row, ScrollStrategy::Center);
                } else {
                    let extra_rows = display_rows[..row].iter().filter(|row| {
                        matches!(row, DiffDisplayRow::Code { source_row }
                            if matches!(file.rows[*source_row].change, crate::command::diff::types::ChangeType::Modified))
                    }).count();
                    let added_side = matches!(display_rows[row], DiffDisplayRow::Code { source_row }
                        if matches!(file.rows[source_row].change, crate::command::diff::types::ChangeType::Modified));
                    self.unified_diff_scroll.scroll_to_item_strict(
                        row + extra_rows + usize::from(added_side), ScrollStrategy::Center,
                    );
                }
            }
        }
        cx.notify();
    }

    fn comment_icon(color: u32) -> impl IntoElement {
        svg()
            .path("icons/comment.svg")
            .size(px(14.))
            .text_color(rgb(color))
    }

    fn comment_action(
        id: &'static str,
        icon: &'static str,
        label: &'static str,
        color: u32,
        enabled: bool,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(26.))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .rounded_sm()
            .text_xs()
            .text_color(rgb(if enabled { color } else { 0x697383 }))
            .when(enabled, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(rgb(0x3a4350)))
            })
            .child(svg().path(icon).size(px(14.)).text_color(rgb(if enabled {
                color
            } else {
                0x697383
            })))
            .child(label)
    }

    pub(super) fn comments_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("toggle-comments")
            .h_full()
            .w(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor_pointer()
            .when(self.review_comments.active, |button| {
                button.bg(rgb(0x35425a))
            })
            .hover(|button| button.bg(rgb(0x35425a)))
            .tooltip(|_, cx| {
                cx.new(|_| ReviewTooltip("Show or hide comments".into()))
                    .into()
            })
            .on_click(cx.listener(|this, _, _, cx| {
                if this.review_comments.active {
                    this.close_comments(cx);
                } else {
                    this.open_comments(cx);
                }
            }))
            .child(Self::comment_icon(if self.review_comments.active {
                YELLOW
            } else {
                MUTED
            }))
    }

    pub(super) fn comments_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let comments = self.current_comments();
        let enabled = !comments.is_empty();
        div().id("comments-side-panel").absolute().top(px(38.)).bottom(px(26.)).right_0()
            .w(px(340.)).max_w_full().flex().flex_col().border_l_1().border_color(rgb(BORDER))
            .bg(rgb(BG)).shadow_lg()
            .child(div().h(px(68.)).flex_none().flex().flex_col().border_b_1().border_color(rgb(BORDER))
                .child(div().h(px(36.)).px_4().flex().items_center().gap_2()
                    .child(Self::comment_icon(YELLOW))
                    .child(div().text_size(px(12.)).text_color(rgb(TEXT)).child("Comments"))
                    .child(div().flex_1().text_xs().text_color(rgb(MUTED)).child(comments.len().to_string()))
                    .child(div().id("close-comments-panel").size(px(22.)).flex().items_center().justify_center()
                        .rounded_sm().cursor_pointer().text_size(px(16.)).text_color(rgb(MUTED))
                        .hover(|button| button.bg(rgb(0x465166)).text_color(rgb(TEXT)))
                        .on_click(cx.listener(|this, _, _, cx| this.close_comments(cx))).child("×")))
                .child(div().h(px(32.)).px_3().flex().items_center().gap_2()
                    .child(div().flex_1().text_xs().text_color(rgb(MUTED)).child("This session"))
                    .child(Self::comment_action("copy-review-comments", "icons/copy.svg", if self.review_comments.copied { "Copied" } else { "Copy" }, if self.review_comments.copied { GREEN } else { TEXT }, enabled)
                        .tooltip(|_, cx| cx.new(|_| ReviewTooltip("Copy all comments with a follow-up prompt for your coding agent".into())).into())
                        .when(enabled, |button| button.on_click(cx.listener(|this, _, _, cx| this.copy_review_comments(cx)))))
                    .child(div().w(px(1.)).h(px(16.)).bg(rgb(BORDER)))
                    .child(Self::comment_action("clear-review-comments", "icons/trash.svg", "Clear", TEXT, enabled)
                        .tooltip(|_, cx| cx.new(|_| ReviewTooltip("Clear all comments and yellow line markers in this repository".into())).into())
                        .when(enabled, |button| button.on_click(cx.listener(|this, _, _, cx| this.clear_review_comments(cx)))))))
            .child(div().id("comments-scroll").flex_1().min_h(px(0.)).overflow_y_scroll().p_4()
                .when(!enabled, |body| body.child(div().pt(px(64.)).flex().flex_col().items_center().gap_3().text_xs().text_color(rgb(MUTED))
                    .child(Self::comment_icon(YELLOW)).child("No review comments yet")
                    .child("Select working-copy lines, then right-click to add a comment.")
                    .child("⌘-click adds separate lines · Shift-click selects a range.")))
                .children(comments.iter().enumerate().map(|(index, comment)| {
                    let target = comment.target.clone();
                    div().mb_3().w_full().rounded_md().border_1().border_color(rgb(BORDER)).bg(rgb(PANEL)).overflow_hidden()
                        .child(div().id(("comment-location", index)).px_3().py_2().flex().items_center().gap_2()
                            .border_b_1().border_color(rgb(BORDER)).text_xs().text_color(rgb(YELLOW)).cursor_pointer()
                            .hover(|row| row.bg(rgb(0x3a4350)))
                            .on_click(cx.listener(move |this, _, _, cx| this.show_comment_source(&target, cx)))
                            .child(Self::comment_icon(YELLOW)).child(div().min_w(px(0.)).child(comment.target.location())))
                        .child(div().px_3().py_3().text_size(px(12.)).text_color(rgb(TEXT))
                            .children(comment.body.split('\n').map(|line| div().min_h(px(18.)).child(line.to_string()))))
                })))
            .with_animation(
                "comments-slide-in",
                Animation::new(Duration::from_millis(180)).with_easing(gpui::ease_out_quint()),
                |panel, progress| panel.right(px(-340. * (1. - progress))),
            )
    }

    pub(super) fn comment_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.review_comments.draft.as_ref().unwrap();
        let enabled = !self.comment_input.read(cx).content().trim().is_empty();
        div()
            .id("comment-dialog-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::rgba(0x14182099))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(560.))
                    .max_w_full()
                    .mx_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x756044))
                    .bg(rgb(PANEL))
                    .shadow_lg()
                    .overflow_hidden()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .border_b_1()
                            .border_color(rgb(BORDER))
                            .text_xs()
                            .child(Self::comment_icon(YELLOW))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_color(rgb(YELLOW))
                                    .child(target.location()),
                            )
                            .child(
                                div()
                                    .id("dismiss-comment")
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .hover(|button| button.bg(rgb(0x465166)))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.cancel_review_comment(cx)
                                        }),
                                    )
                                    .child("×"),
                            ),
                    )
                    .child(
                        div()
                            .m_3()
                            .p_2()
                            .h(px(152.))
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(BG))
                            .text_size(px(13.))
                            .child(self.comment_input.clone()),
                    )
                    .child(
                        div()
                            .px_3()
                            .pb_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(rgb(MUTED))
                                    .child("⌘ Enter to add · Esc to cancel"),
                            )
                            .child(
                                div()
                                    .id("cancel-comment")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .hover(|button| button.bg(rgb(0x3a4350)))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.cancel_review_comment(cx)
                                        }),
                                    )
                                    .child("Cancel"),
                            )
                            .child(
                                div()
                                    .id("save-comment")
                                    .px_3()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgb(if enabled { 0xe5b567 } else { 0x4b463c }))
                                    .text_color(rgb(if enabled { 0x242933 } else { MUTED }))
                                    .when(enabled, |button| {
                                        button
                                            .cursor_pointer()
                                            .hover(|button| button.bg(rgb(0xf0c47f)))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let body = this
                                                    .comment_input
                                                    .read(cx)
                                                    .content()
                                                    .to_string();
                                                this.save_review_comment(&body, cx);
                                            }))
                                    })
                                    .child("Add comment"),
                            ),
                    ),
            )
    }
}
