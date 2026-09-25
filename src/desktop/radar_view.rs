//! Compact native Radar tab. Analysis/layout live outside the render path.

use gpui::{canvas, AnyElement, PathBuilder};

use super::*;

fn color(change: radar::Change) -> u32 {
    match change {
        radar::Change::Added => GREEN,
        radar::Change::Removed => RED,
        radar::Change::Modified => BLUE,
        radar::Change::Unchanged => MUTED,
    }
}

impl ReviewWorkspace {
    pub(super) fn open_radar(&mut self, cx: &mut Context<Self>) {
        self.radar.open = true;
        self.radar.active = true;
        self.radar.use_bubbles = true;
        self.active_log = None;
        if !self.radar.has_graph() && !self.radar.loading {
            self.radar.refresh(
                self.repository_root.clone(),
                self.reference.clone(),
                self.file_filters.clone(),
            );
        }
        if self.radar_bubbles.map.is_none() && !self.radar_bubbles.loading {
            self.refresh_radar_bubbles();
        }
        self.persist_project_settings();
        cx.notify();
    }

    fn radar_context_icon(expanded: bool) -> impl IntoElement {
        svg()
            .path(if expanded {
                "icons/collapse_unchanged.svg"
            } else {
                "icons/expand_unchanged.svg"
            })
            .size(px(16.))
            .flex_none()
            .text_color(rgb(MUTED))
    }

    fn radar_dependency_icon() -> impl IntoElement {
        svg()
            .path("icons/focus.svg")
            .size(px(14.))
            .flex_none()
            .text_color(rgb(TEXT))
    }

    fn choose_radar_function(
        &mut self,
        path: &str,
        line: usize,
        change: radar::Change,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.model.files.iter().position(|file| file.path == path) else {
            return;
        };
        self.choose_file(index, true, cx);
        let Some(file) = self.selected_file() else {
            return;
        };
        let Some(source_row) = file.rows.iter().position(|row| {
            if change == radar::Change::Removed {
                row.old_number == Some(line)
            } else {
                row.new_number == Some(line)
            }
        }) else {
            return;
        };
        self.collapsed_unchanged_sections.retain(|section| {
            section.file_path != path
                || source_row < section.start_row
                || source_row >= section.end_row
        });
        if let Some(file) = self.selected_file() {
            if let Some(row)=display_rows_for_file(file,&self.collapsed_unchanged_sections,self.lsp_symbols.get(&file.path).map(Vec::as_slice).unwrap_or(&[])).iter().position(|row|matches!(row,DiffDisplayRow::Code{source_row: candidate} if *candidate==source_row)) {
                self.diff_list_scroll.scroll_to_item_strict(row,ScrollStrategy::Center);
            }
        }
        cx.notify();
    }

    fn radar_legacy_canvas(&self, cx: &mut Context<Self>) -> AnyElement {
        let message = if self.radar.loading && self.radar.diagram.is_none() {
            Some("Loading dependencies…".to_string())
        } else if let Some(error) = &self.radar.error {
            Some(format!("Unable to load Radar: {error}"))
        } else if self
            .radar
            .diagram
            .as_ref()
            .is_none_or(|diagram| diagram.groups.is_empty())
        {
            Some("No connected JavaScript / TypeScript changes".to_string())
        } else {
            None
        };
        let all_expanded = self.radar.diagram.as_ref().is_some_and(|d| d.all_expanded);
        let has_context = self
            .radar
            .diagram
            .as_ref()
            .is_some_and(|d| d.total_chains > 0);
        let dependency_focus = self.radar.dependency_focus();
        let focused_files = &self.radar.focused_files;
        let focused_count = focused_files.len();
        let mut content = div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .child(
                div()
                    .h(px(36.))
                    .flex_none()
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child("File dependencies"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .when(focused_count > 0, |element| {
                                element.child(
                                    div()
                                        .id("radar-clear-dependency-focus")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px_2()
                                        .py_1()
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(rgb(0x657081))
                                        .bg(rgb(PANEL))
                                        .text_size(px(11.))
                                        .text_color(rgb(TEXT))
                                        .cursor_pointer()
                                        .hover(|e| e.bg(rgb(0x3b4350)))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.radar.clear_dependency_focus();
                                            cx.notify();
                                        }))
                                        .child(Self::radar_dependency_icon())
                                        .child(format!("{focused_count} selected · Clear")),
                                )
                            })
                            .child(
                                div()
                                    .id("radar-toggle-context")
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .text_size(px(11.))
                                    .text_color(rgb(if has_context { TEXT } else { MUTED }))
                                    .when(has_context, |element| {
                                        element
                                            .cursor_pointer()
                                            .hover(|e| e.bg(rgb(PANEL)))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.radar.toggle_all();
                                                this.persist_project_settings();
                                                cx.notify();
                                            }))
                                    })
                                    .child(Self::radar_context_icon(all_expanded))
                                    .child(if all_expanded && has_context {
                                        "Collapse context"
                                    } else {
                                        "Expand context"
                                    }),
                            ),
                    ),
            );
        if let Some(message) = message {
            content = content.child(
                div()
                    .flex_1()
                    .p_6()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(message),
            );
        } else if let Some(diagram) = &self.radar.diagram {
            let drawing = diagram.clone();
            let drawing_focus = dependency_focus.clone();
            let mut graph = div()
                .relative()
                .flex_none()
                .w(px(diagram.width))
                .h(px(diagram.height));
            for cycle in &diagram.cycles {
                graph = graph.child(
                    div()
                        .absolute()
                        .left(px(cycle.position.x))
                        .top(px(cycle.position.y))
                        .w(px(cycle.width))
                        .h(px(cycle.height))
                        .border_1()
                        .border_color(rgb(0x596372))
                        .rounded_sm()
                        .bg(rgb(0x282e38)),
                );
            }
            graph = graph.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        // Paint neutral routes first, isolate highlighted routes
                        // from anything beneath them, then paint their colors.
                        for phase in 0..3 {
                            for edge in &drawing.connections {
                                let focused = drawing_focus.as_ref().is_some_and(|focus| {
                                    focus.edges.iter().any(|(from, to)| {
                                        edge.from.contains(from) && edge.to.contains(to)
                                    })
                                });
                                let focus_active = drawing_focus.is_some();
                                let underlay = focus_active && focused && phase == 1;
                                let should_paint = if focus_active {
                                    (phase == 0 && !focused) || (phase > 0 && focused)
                                } else {
                                    phase == 0
                                };
                                if !should_paint {
                                    continue;
                                }

                                let mut path = PathBuilder::stroke(px(if underlay {
                                    3.6
                                } else if focused {
                                    1.8
                                } else if focus_active {
                                    1.
                                } else {
                                    1.5
                                }));
                                if !underlay && edge.change == radar::Change::Removed {
                                    path = path.dash_array(&[px(4.), px(3.)]);
                                }
                                for (i, p) in edge.points.iter().enumerate() {
                                    let p = bounds.origin + point(px(p.x), px(p.y));
                                    if i == 0 {
                                        path.move_to(p);
                                    } else {
                                        path.line_to(p);
                                    }
                                }
                                let edge_color = if underlay {
                                    BG
                                } else if focus_active && !focused {
                                    0x414957
                                } else if focused {
                                    TEXT
                                } else if edge.change == radar::Change::Added {
                                    TEXT
                                } else if edge.change == radar::Change::Unchanged {
                                    0x657081
                                } else {
                                    color(edge.change)
                                };
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, rgb(edge_color));
                                }
                                if underlay {
                                    continue;
                                }
                                if let Some((last, rest)) = edge.points.split_last() {
                                    if let Some(previous) =
                                        rest.iter().rev().find(|p| p.x != last.x || p.y != last.y)
                                    {
                                        let dx = last.x - previous.x;
                                        let dy = last.y - previous.y;
                                        let length = dx.hypot(dy);
                                        let (ux, uy) = (dx / length, dy / length);
                                        let tip = bounds.origin + point(px(last.x), px(last.y));
                                        let mut arrow = PathBuilder::fill();
                                        arrow.move_to(tip);
                                        arrow.line_to(
                                            tip + point(
                                                px(-ux * 7. - uy * 3.5),
                                                px(-uy * 7. + ux * 3.5),
                                            ),
                                        );
                                        arrow.line_to(
                                            tip + point(
                                                px(-ux * 7. + uy * 3.5),
                                                px(-uy * 7. - ux * 3.5),
                                            ),
                                        );
                                        arrow.close();
                                        if let Ok(arrow) = arrow.build() {
                                            window.paint_path(arrow, rgb(edge_color));
                                        }
                                    }
                                }
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
            for (index, cycle) in diagram.cycles.iter().enumerate() {
                let key = cycle.key.clone();
                graph = graph.child(
                    div()
                        .id(("radar-cycle", index))
                        .absolute()
                        .left(px(cycle.position.x + 8.))
                        .top(px(cycle.position.y + 3.))
                        .w(px(cycle.width - 16.))
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.radar.toggle_cycle(key.clone());
                            this.persist_project_settings();
                            cx.notify();
                        }))
                        .child(Self::radar_context_icon(cycle.expanded))
                        .child(format!("{} files · dependency cycle", cycle.count)),
                );
            }
            let mut node_index = 0_usize;
            for (index, group) in diagram.groups.iter().enumerate() {
                let context = group.change == radar::Change::Unchanged;
                let dependency_focused = focused_files.contains(&group.path);
                let selection_color = color(group.change);
                let selection_background = match group.change {
                    radar::Change::Added => 0x354d46,
                    radar::Change::Modified => 0x35445c,
                    radar::Change::Removed => 0x503b44,
                    radar::Change::Unchanged => 0x3b4654,
                };
                let tooltip = group.path.clone();
                let path = group.path.clone();
                let navigable = self.model.files.iter().any(|file| file.path == path);
                let mut card = div()
                    .id(("radar-file", index))
                    .absolute()
                    .left(px(group.position.x))
                    .top(px(group.position.y))
                    .w(px(group.width))
                    .h(px(group.height))
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(if dependency_focused {
                        selection_color
                    } else if context {
                        BORDER
                    } else {
                        color(group.change)
                    }))
                    .bg(rgb(if dependency_focused {
                        selection_background
                    } else if context {
                        BG
                    } else {
                        0x303640
                    }))
                    .when(dependency_focused, |element| element.shadow_lg())
                    .tooltip(move |_, cx| cx.new(|_| RadarTooltip(tooltip.clone())).into())
                    .when(dependency_focused, |element| {
                        element.child(
                            div()
                                .absolute()
                                .left(px(0.))
                                .top(px(0.))
                                .bottom(px(0.))
                                .w(px(4.))
                                .bg(rgb(selection_color)),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(10.))
                            .top(px(6.))
                            .right(px(if dependency_focused {
                                104.
                            } else if group.counts.iter().sum::<usize>() > 0 {
                                32.
                            } else {
                                10.
                            }))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child(
                                div().truncate().child(
                                    Path::new(&group.path)
                                        .parent()
                                        .filter(|p| !p.as_os_str().is_empty())
                                        .map(|p| p.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| group.package.clone()),
                                ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(rgb(color(group.change)))
                                    .child(if context {
                                        if group.entry {
                                            "entry point"
                                        } else if group.root {
                                            "root"
                                        } else {
                                            ""
                                        }
                                    } else {
                                        group.change.marker()
                                    }),
                            ),
                    )
                    .when(dependency_focused, |element| {
                        element.child(
                            div()
                                .absolute()
                                .right(px(34.))
                                .top(px(5.))
                                .px_1()
                                .py(px(1.))
                                .rounded_sm()
                                .bg(rgb(selection_color))
                                .text_size(px(9.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(rgb(0x282d36))
                                .child("SELECTED"),
                        )
                    })
                    .child(
                        div()
                            .id(("radar-file-title", index))
                            .absolute()
                            .left(px(10.))
                            .top(px(22.))
                            .right(px(26.))
                            .truncate()
                            .text_size(px(13.))
                            .text_color(rgb(TEXT))
                            .when(navigable, |e| {
                                e.cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.choose_review_item(&path, None, cx)
                                    }))
                            })
                            .child(group.file_name.clone()),
                    );
                if group.counts.iter().sum::<usize>() > 0 {
                    let path = group.path.clone();
                    let expanded = group.expanded;
                    card = card.child(
                        div()
                            .id(("radar-toggle-file", index))
                            .absolute()
                            .right(px(4.))
                            .top(px(3.))
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|e| e.bg(rgb(0x465166)))
                            .tooltip(move |_, cx| {
                                cx.new(|_| {
                                    RadarTooltip(
                                        if expanded {
                                            "Collapse functions"
                                        } else {
                                            "Expand functions"
                                        }
                                        .into(),
                                    )
                                })
                                .into()
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.radar.toggle_file(path.clone());
                                this.persist_project_settings();
                                cx.notify();
                            }))
                            .child(Self::radar_context_icon(expanded)),
                    );
                    let summary_tooltip = format!("Functions: {} added, {} changed, {} deleted, {} unchanged callers / dependencies",
                        group.counts[0], group.counts[1], group.counts[2], group.counts[3]);
                    let mut summary = div()
                        .id(("radar-function-summary", index))
                        .absolute()
                        .left(px(10.))
                        .top(px(44.))
                        .right(px(24.))
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(11.))
                        .tooltip(move |_, cx| {
                            cx.new(|_| RadarTooltip(summary_tooltip.clone())).into()
                        });
                    for (count, label, tint) in [
                        (group.counts[0], "A", GREEN),
                        (group.counts[1], "M", BLUE),
                        (group.counts[2], "D", RED),
                        (group.counts[3], "context", MUTED),
                    ] {
                        if count > 0 {
                            summary = summary.child(
                                div()
                                    .text_color(rgb(tint))
                                    .child(format!("{count} {label}")),
                            );
                        }
                    }
                    card = card.child(summary);
                }
                let focus_path = group.path.clone();
                card = card.child(
                    div()
                        .id(("radar-focus-dependencies", index))
                        .absolute()
                        .right(px(4.))
                        .bottom(px(4.))
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(if dependency_focused { TEXT } else { 0x596372 }))
                        .bg(rgb(if dependency_focused { 0x4a515d } else { PANEL }))
                        .text_color(rgb(TEXT))
                        .cursor_pointer()
                        .hover(|e| {
                            e.bg(rgb(0x536072))
                                .border_color(rgb(TEXT))
                                .text_color(rgb(TEXT))
                        })
                        .tooltip(move |_, cx| {
                            cx.new(|_| {
                                RadarTooltip(
                                    if dependency_focused {
                                        "Remove this file from dependency focus"
                                    } else {
                                        "Add this file to dependency focus"
                                    }
                                    .into(),
                                )
                            })
                            .into()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.radar.toggle_dependency_focus(focus_path.clone());
                            this.persist_project_settings();
                            cx.notify();
                        }))
                        .child(Self::radar_dependency_icon()),
                );
                for (function_index, function) in group.functions.iter().enumerate() {
                    let path = group.path.clone();
                    let line = function.node.line;
                    let change = function.node.change;
                    let label = format!(
                        "{}{} {}()",
                        if function.depth > 0 { "↳ " } else { "" },
                        change.marker(),
                        function.node.name
                    );
                    let tooltip = if function.calls.is_empty() {
                        label.clone()
                    } else {
                        format!("{label}\nCalls:\n{}", function.calls.join("\n"))
                    };
                    card = card.child(
                        div()
                            .id(("radar-function", node_index))
                            .absolute()
                            .left(px(function.position.x - group.position.x))
                            .top(px(function.position.y - group.position.y))
                            .w(px(function.width
                                - if function_index + 1 == group.functions.len() {
                                    20.
                                } else {
                                    0.
                                }))
                            .h(px(radar::FUNCTION_ROW_HEIGHT))
                            .px_1()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .text_size(px(12.))
                            .text_color(rgb(color(change)))
                            .tooltip(move |_, cx| cx.new(|_| RadarTooltip(tooltip.clone())).into())
                            .when(navigable, |e| {
                                e.cursor_pointer().hover(|e| e.bg(rgb(0x3b4556))).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.choose_radar_function(&path, line, change, cx)
                                    }),
                                )
                            })
                            .child(div().truncate().child(label)),
                    );
                    node_index += 1;
                }
                graph = graph.child(card);
            }
            for (index, control) in diagram.context.iter().enumerate() {
                let key = control.key.clone();
                let tooltip = control.files.join("\n");
                graph = graph.child(
                    div()
                        .id(("radar-context", index))
                        .absolute()
                        .left(px(control.position.x))
                        .top(px(control.position.y))
                        .w(px(control.width))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap_1()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .bg(rgb(BG))
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(|e| e.bg(rgb(PANEL)))
                        .tooltip(move |_, cx| cx.new(|_| RadarTooltip(tooltip.clone())).into())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.radar.toggle_chain(key.clone());
                            this.persist_project_settings();
                            cx.notify();
                        }))
                        .child(Self::radar_context_icon(control.expanded))
                        .child(if control.expanded {
                            "Collapse context".into()
                        } else {
                            format!(
                                "⋯ {} unchanged {}",
                                control.files.len(),
                                if control.files.len() == 1 {
                                    "file"
                                } else {
                                    "files"
                                }
                            )
                        }),
                );
            }
            content = content.child(
                div()
                    .id("radar-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .overflow_x_scroll()
                    .overflow_y_scroll()
                    .scrollbar_width(px(10.))
                    .child(graph),
            );
        }
        content.into_any_element()
    }
}

struct RadarTooltip(String);
impl Render for RadarTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_sm()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL))
            .text_size(px(11.))
            .text_color(rgb(TEXT))
            .child(self.0.clone())
    }
}
