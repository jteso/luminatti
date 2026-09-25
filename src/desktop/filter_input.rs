use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    GlobalElementId, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, PaintQuad, Pixels, Point,
    SharedString, Style, TextAlign, TextRun, UTF16Selection, Window, WrappedLine,
};

use super::{rgb, theme, BLUE, MUTED, TEXT, Copy, Cut, Paste, SelectAll};

const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);

pub(super) enum FileFilterInputEvent {
    Submit(String),
    Invalid(String),
    Changed,
    Dismiss,
}

pub(super) struct FileFilterInput {
    focus_handle: FocusHandle,
    content: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Rc<Vec<WrappedLine>>,
    last_origin: Point<Pixels>,
    last_line_height: Pixels,
    multiline: bool,
    last_bounds: Option<Bounds<Pixels>>,
    cursor_visible: bool,
    cursor_blink_enabled: bool,
    cursor_blink_epoch: usize,
}

impl FileFilterInput {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        cx.on_focus(&focus_handle, window, Self::handle_focus)
            .detach();
        cx.on_blur(&focus_handle, window, Self::handle_blur)
            .detach();

        Self {
            focus_handle,
            content: "".into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: Rc::new(Vec::new()),
            last_origin: point(px(0.), px(0.)),
            last_line_height: px(20.),
            multiline: false,
            last_bounds: None,
            cursor_visible: false,
            cursor_blink_enabled: false,
            cursor_blink_epoch: 0,
        }
    }

    /// Reuse the native input/IME implementation for the multiline review composer.
    pub(super) fn new_comment(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            multiline: true,
            ..Self::new(window, cx)
        }
    }

    pub(super) fn content(&self) -> &str {
        &self.content
    }

    pub(super) fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.pause_cursor_blinking(cx);
        cx.notify();
    }

    fn handle_focus(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.enable_cursor_blinking(cx);
    }

    fn handle_blur(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.cursor_visible = false;
        self.cursor_blink_enabled = false;
    }

    fn next_cursor_blink_epoch(&mut self) -> usize {
        self.cursor_blink_epoch = self.cursor_blink_epoch.wrapping_add(1);
        self.cursor_blink_epoch
    }

    fn enable_cursor_blinking(&mut self, cx: &mut Context<Self>) {
        if self.cursor_blink_enabled {
            return;
        }

        self.cursor_blink_enabled = true;
        self.cursor_visible = false;
        self.blink_cursor(self.cursor_blink_epoch, cx);
    }

    fn blink_cursor(&mut self, epoch: usize, cx: &mut Context<Self>) {
        if epoch != self.cursor_blink_epoch || !self.cursor_blink_enabled {
            return;
        }

        self.cursor_visible = !self.cursor_visible;
        cx.notify();

        let epoch = self.next_cursor_blink_epoch();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CURSOR_BLINK_INTERVAL).await;
            if let Some(this) = this.upgrade() {
                let _ = this.update(cx, |this, cx| this.blink_cursor(epoch, cx));
            }
        })
        .detach();
    }

    fn pause_cursor_blinking(&mut self, cx: &mut Context<Self>) {
        if !self.cursor_blink_enabled {
            return;
        }

        if !self.cursor_visible {
            self.cursor_visible = true;
            cx.notify();
        }

        let epoch = self.next_cursor_blink_epoch();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CURSOR_BLINK_INTERVAL).await;
            if let Some(this) = this.upgrade() {
                let _ = this.update(cx, |this, cx| {
                    if epoch == this.cursor_blink_epoch && this.cursor_blink_enabled {
                        this.blink_cursor(epoch, cx);
                    }
                });
            }
        })
        .detach();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.pause_cursor_blinking(cx);
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for character in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += character.len_utf16();
            utf8_offset += character.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for character in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += character.len_utf8();
            utf16_offset += character.len_utf16();
        }
        utf16_offset
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let mut start = 0;
        let mut y = self.last_origin.y;
        for line in self.last_layout.iter() {
            let height = line.size(self.last_line_height).height;
            if position.y < y + height {
                let index = line
                    .closest_index_for_position(
                        point(
                            position.x - self.last_origin.x,
                            (position.y - y).max(px(0.)),
                        ),
                        self.last_line_height,
                    )
                    .unwrap_or_else(|index| index);
                return (start + index).min(self.content.len());
            }
            start += line.len() + 1;
            y += height;
        }
        self.content.len()
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let anchor = if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        self.selected_range = anchor.min(offset)..anchor.max(offset);
        self.selection_reversed = offset < anchor;
        self.pause_cursor_blinking(cx);
        cx.notify();
    }

    fn replace_selection(&mut self, text: &str, cx: &mut Context<Self>) {
        let range = self.selected_range.clone();
        self.content =
            (self.content[0..range.start].to_owned() + text + &self.content[range.end..]).into();
        let cursor = range.start + text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.pause_cursor_blinking(cx);
        cx.emit(FileFilterInputEvent::Changed);
        cx.notify();
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn copy_selection(&self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            self.copy_selection(cx);
            self.replace_selection("", cx);
        }
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.multiline {
                text.replace("\r\n", "\n").replace('\r', "\n")
            } else {
                text.replace(['\r', '\n'], "")
            };
            self.replace_selection(&text, cx);
        }
    }

    fn select_all_action(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_all(cx);
    }

    fn copy_action(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    fn cut_action(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        self.cut_selection(cx);
    }

    fn paste_action(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_clipboard(cx);
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = &event.keystroke;
        match key.key.as_str() {
            "enter" if self.multiline => {
                if key.modifiers.secondary() {
                    if !self.content.trim().is_empty() {
                        cx.emit(FileFilterInputEvent::Submit(
                            self.content.trim().to_string(),
                        ));
                    }
                } else {
                    self.replace_selection("\n", cx);
                }
                cx.stop_propagation();
            }
            "enter" => {
                let pattern = self.content.trim();
                if pattern.is_empty() {
                    cx.emit(FileFilterInputEvent::Invalid(
                        "Enter a glob pattern first.".to_string(),
                    ));
                } else {
                    match globset::Glob::new(pattern) {
                        Ok(_) => {
                            let pattern = pattern.to_string();
                            self.clear(cx);
                            cx.emit(FileFilterInputEvent::Submit(pattern));
                        }
                        Err(error) => {
                            cx.emit(FileFilterInputEvent::Invalid(error.to_string()));
                        }
                    }
                }
                cx.stop_propagation();
            }
            "escape" => {
                cx.emit(FileFilterInputEvent::Dismiss);
                cx.stop_propagation();
            }
            "backspace" => {
                if self.selected_range.is_empty() {
                    let cursor = self.cursor_offset();
                    let previous = self.previous_boundary(cursor);
                    self.selected_range = previous..cursor;
                }
                if !self.selected_range.is_empty() {
                    self.replace_selection("", cx);
                }
                cx.stop_propagation();
            }
            "delete" => {
                if self.selected_range.is_empty() {
                    let cursor = self.cursor_offset();
                    let next = self.next_boundary(cursor);
                    self.selected_range = cursor..next;
                }
                if !self.selected_range.is_empty() {
                    self.replace_selection("", cx);
                }
                cx.stop_propagation();
            }
            "left" => {
                let offset = if key.modifiers.shift || self.selected_range.is_empty() {
                    self.previous_boundary(self.cursor_offset())
                } else {
                    self.selected_range.start
                };
                if key.modifiers.shift {
                    self.select_to(offset, cx);
                } else {
                    self.move_to(offset, cx);
                }
                cx.stop_propagation();
            }
            "right" => {
                let offset = if key.modifiers.shift || self.selected_range.is_empty() {
                    self.next_boundary(self.cursor_offset())
                } else {
                    self.selected_range.end
                };
                if key.modifiers.shift {
                    self.select_to(offset, cx);
                } else {
                    self.move_to(offset, cx);
                }
                cx.stop_propagation();
            }
            "home" | "end" | "up" | "down" => {
                let cursor = self.cursor_offset();
                let offset = match key.key.as_str() {
                    "home" => self.content[..cursor]
                        .rfind('\n')
                        .map(|index| index + 1)
                        .unwrap_or(0),
                    "end" => self.content[cursor..]
                        .find('\n')
                        .map(|index| cursor + index)
                        .unwrap_or(self.content.len()),
                    direction => {
                        let position =
                            layout_position(&self.last_layout, cursor, self.last_line_height)
                                + self.last_origin;
                        self.index_for_mouse_position(
                            position
                                + point(
                                    px(0.),
                                    if direction == "up" {
                                        -self.last_line_height
                                    } else {
                                        self.last_line_height
                                    },
                                ),
                        )
                    }
                };
                if key.modifiers.shift {
                    self.select_to(offset, cx);
                } else {
                    self.move_to(offset, cx);
                }
                cx.stop_propagation();
            }
            "a" if key.modifiers.secondary() => {
                self.select_all(cx);
                cx.stop_propagation();
            }
            "c" if key.modifiers.secondary() => {
                self.copy_selection(cx);
                cx.stop_propagation();
            }
            "x" if key.modifiers.secondary() => {
                self.cut_selection(cx);
                cx.stop_propagation();
            }
            "v" if key.modifiers.secondary() => {
                self.paste_clipboard(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
        cx.stop_propagation();
    }
}

impl gpui::EventEmitter<FileFilterInputEvent> for FileFilterInput {}

impl EntityInputHandler for FileFilterInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(range) = range_utf16 {
            self.selected_range = self.range_from_utf16(&range);
        } else if let Some(range) = self.marked_range.take() {
            self.selected_range = range;
        }
        self.replace_selection(text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + text + &self.content[range.end..]).into();
        self.marked_range = (!text.is_empty()).then_some(range.start..range.start + text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .map(|selection| range.start + selection.start..range.start + selection.end)
            .unwrap_or_else(|| range.start + text.len()..range.start + text.len());
        cx.emit(FileFilterInputEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let start = layout_position(&self.last_layout, range.start, self.last_line_height)
            + self.last_origin;
        let end =
            layout_position(&self.last_layout, range.end, self.last_line_height) + self.last_origin;
        let _ = bounds;
        Some(Bounds::new(
            start,
            size((end.x - start.x).max(px(1.)), self.last_line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse_position(point)))
    }
}

struct FileFilterInputElement {
    input: Entity<FileFilterInput>,
}

struct InputPaintState {
    lines: Rc<Vec<WrappedLine>>,
    cursor: Option<PaintQuad>,
    origin: Point<Pixels>,
    line_height: Pixels,
}

fn layout_position(lines: &[WrappedLine], offset: usize, line_height: Pixels) -> Point<Pixels> {
    let mut start = 0;
    let mut y = px(0.);
    for line in lines {
        if offset <= start + line.len() {
            return line
                .position_for_index(offset - start, line_height)
                .unwrap_or_default()
                + point(px(0.), y);
        }
        start += line.len() + 1;
        y += line.size(line_height).height;
    }
    point(px(0.), y)
}

impl IntoElement for FileFilterInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FileFilterInputElement {
    type RequestLayoutState = ();
    type PrepaintState = InputPaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let focused = input.focus_handle.is_focused(window);
        let placeholder = input.content.is_empty();
        let text: SharedString = if placeholder {
            if input.multiline {
                "Ask a question or leave feedback…".into()
            } else {
                "Glob pattern, e.g. *.test.ts".into()
            }
        } else {
            input.content.clone()
        };
        let style = window.text_style();
        let selection = if placeholder {
            0..0
        } else {
            input.selected_range.clone()
        };
        let runs = [
            (0..selection.start, false),
            (selection.clone(), true),
            (selection.end..text.len(), false),
        ]
        .into_iter()
        .filter(|(range, _)| !range.is_empty())
        .map(|(range, selected)| TextRun {
            len: range.len(),
            font: style.font(),
            color: rgb(if placeholder { MUTED } else { TEXT }).into(),
            background_color: selected.then(|| theme::rgba(0x7aa2f744).into()),
            underline: None,
            strikethrough: None,
        })
        .collect::<Vec<_>>();
        let lines = Rc::new(
            window
                .text_system()
                .shape_text(
                    text,
                    style.font_size.to_pixels(window.rem_size()),
                    &runs,
                    input.multiline.then_some(bounds.size.width.max(px(1.))),
                    None,
                )
                .unwrap_or_default()
                .into_vec(),
        );
        let line_height = px(20.);
        let caret = layout_position(&lines, input.cursor_offset(), line_height);
        // Keep the caret visible while typing or navigating long comments.
        let previous_scroll = input
            .last_bounds
            .map(|last| last.top() + px(4.) - input.last_origin.y)
            .unwrap_or_default();
        let visible_height = (bounds.size.height - px(8.)).max(line_height);
        let total_height = lines
            .iter()
            .map(|line| line.size(line_height).height)
            .fold(px(0.), |a, b| a + b);
        let mut scroll = previous_scroll.clamp(px(0.), (total_height - visible_height).max(px(0.)));
        if caret.y < scroll {
            scroll = caret.y;
        }
        if caret.y + line_height > scroll + visible_height {
            scroll = caret.y + line_height - visible_height;
        }
        let x_scroll = if input.multiline {
            px(0.)
        } else {
            (caret.x - bounds.size.width + px(2.)).max(px(0.))
        };
        let origin = point(bounds.left() - x_scroll, bounds.top() + px(4.) - scroll);
        let cursor = (focused && input.cursor_visible).then(|| {
            fill(
                Bounds::new(origin + caret, size(px(1.), line_height)),
                rgb(BLUE),
            )
        });
        InputPaintState {
            lines,
            cursor,
            origin,
            line_height,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            let mut origin = state.origin;
            for line in state.lines.iter() {
                let _ = line.paint_background(
                    origin,
                    state.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
                let _ = line.paint(origin, state.line_height, TextAlign::Left, None, window, cx);
                origin.y += line.size(state.line_height).height;
            }
            if let Some(cursor) = state.cursor.take() {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, _| {
            input.last_layout = state.lines.clone();
            input.last_bounds = Some(bounds);
            input.last_origin = state.origin;
            input.last_line_height = state.line_height;
        });
    }
}

impl Render for FileFilterInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::select_all_action))
            .on_action(cx.listener(Self::copy_action))
            .on_action(cx.listener(Self::cut_action))
            .on_action(cx.listener(Self::paste_action))
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                if event.dragging() {
                    this.select_to(this.index_for_mouse_position(event.position), cx);
                    cx.stop_propagation();
                }
            }))
            .child(FileFilterInputElement {
                input: cx.entity().clone(),
            })
    }
}
