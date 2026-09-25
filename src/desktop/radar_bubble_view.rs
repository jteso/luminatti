//! Interactive repository map. One canvas paints the visible circles; labels
//! and the inspector are small native elements above it.

use gpui::{canvas, quad, AnyElement, BorderStyle, PathBuilder};

use super::*;

fn bubble_color(change: radar::Change) -> u32 {
    match change {
        radar::Change::Added => 0x81cb91,
        radar::Change::Modified => 0x80aafa,
        radar::Change::Removed => 0xe88389,
        radar::Change::Unchanged => 0x62666d,
    }
}

impl ReviewWorkspace {
    fn radar_set_changed_only(&mut self, changed_only: bool, cx: &mut Context<Self>) {
        self.radar_bubbles.view_menu_open = false;
        if self.radar_bubbles.show_changed_only == changed_only {
            cx.notify();
            return;
        }
        self.radar_bubbles.show_changed_only = changed_only;
        self.radar_bubbles.clear_hover_emphasis();
        self.radar_bubbles.selected = None;
        self.radar_bubbles.focus = None;
        self.radar_bubbles.history.clear();
        self.radar_bubbles.show_all_dependencies = false;
        self.radar_bubbles.layout_epoch = self.radar_bubbles.layout_epoch.wrapping_add(1);
        self.radar_bubbles.reveal_epoch = self.radar_bubbles.reveal_epoch.wrapping_add(1);
        self.radar_move_camera(1., (0., 0.), cx);
    }

    fn radar_move_camera(
        &mut self,
        target_zoom: f32,
        target_pan: (f32, f32),
        cx: &mut Context<Self>,
    ) {
        let start_pan = self.radar_bubbles.pan;
        let start_zoom = self.radar_bubbles.zoom;
        self.radar_bubbles.camera_epoch = self.radar_bubbles.camera_epoch.wrapping_add(1);
        let epoch = self.radar_bubbles.camera_epoch;
        cx.spawn(async move |this, cx| {
            for step in 1..=20 {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if let Some(this) = this.upgrade() {
                    let _ = this.update(cx, |this, cx| {
                        if this.radar_bubbles.camera_epoch != epoch {
                            return;
                        }
                        let t = step as f32 / 20.;
                        let ease = t * t * (3. - 2. * t);
                        this.radar_bubbles.zoom = start_zoom + (target_zoom - start_zoom) * ease;
                        this.radar_bubbles.pan = (
                            start_pan.0 + (target_pan.0 - start_pan.0) * ease,
                            start_pan.1 + (target_pan.1 - start_pan.1) * ease,
                        );
                        cx.notify();
                    });
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn radar_focus(&mut self, path: String, cx: &mut Context<Self>) {
        self.radar_bubbles.clear_hover_emphasis();
        let Some(map) = self.radar_bubbles.current_map() else {
            return;
        };
        let Some(node) = map.nodes.iter().find(|n| n.path == path) else {
            self.radar_bubbles.selected = None;
            self.radar_bubbles.focus = None;
            self.radar_bubbles.layout_epoch = self.radar_bubbles.layout_epoch.wrapping_add(1);
            self.radar_bubbles.reveal_epoch = self.radar_bubbles.reveal_epoch.wrapping_add(1);
            self.radar_move_camera(1., (0., 0.), cx);
            return;
        };
        let (_, _, width, height) = *self.radar_bubbles.viewport.lock().unwrap();
        let fit = (width / (2. * map.radius + 48.))
            .min(height / (2. * map.radius + 48.))
            .max(0.0001);
        let (target_zoom, target_pan, focus) = if node.directory {
            let zoom = (width.min(height) * 0.32 / node.radius / fit).clamp(1., 100.);
            (zoom, (-node.x * fit * zoom, -node.y * fit * zoom), None)
        } else {
            let changed = map
                .nodes
                .iter()
                .filter(|node| !node.directory && node.change != radar::Change::Unchanged)
                .map(|node| node.path.clone())
                .collect::<std::collections::BTreeSet<_>>();
            let mut dependencies = if self.radar_bubbles.show_all_dependencies {
                self.radar.focus_for(&path)
            } else {
                self.radar
                    .review_focus_for(&path, &changed, self.radar_bubbles.show_changed_only)
            }
            .unwrap_or_default();
            let visible = map
                .nodes
                .iter()
                .filter(|node| !node.directory)
                .map(|node| node.path.as_str())
                .collect::<HashSet<_>>();
            dependencies
                .files
                .retain(|file| visible.contains(file.as_str()));
            dependencies.edges.retain(|(from, to)| {
                visible.contains(from.as_str()) && visible.contains(to.as_str())
            });
            dependencies.files.insert(path.clone());
            if self.radar_bubbles.show_changed_only {
                // Removing unchanged intermediaries can split a route. Keep only
                // the component that still connects to the selected file.
                let mut adjacent: HashMap<&str, Vec<&str>> = HashMap::new();
                for (from, to) in &dependencies.edges {
                    adjacent.entry(from).or_default().push(to);
                    adjacent.entry(to).or_default().push(from);
                }
                let mut reachable = std::collections::BTreeSet::from([path.clone()]);
                let mut queue = std::collections::VecDeque::from([path.as_str()]);
                while let Some(current) = queue.pop_front() {
                    for &neighbor in adjacent.get(current).into_iter().flatten() {
                        if reachable.insert(neighbor.to_string()) {
                            queue.push_back(neighbor);
                        }
                    }
                }
                dependencies.files.retain(|file| reachable.contains(file));
                dependencies
                    .edges
                    .retain(|(from, to)| reachable.contains(from) && reachable.contains(to));
            }
            let mut focus = radar::bubbles::BubbleFocus::new(&path, dependencies);
            focus.arrange(
                map,
                &path,
                fit,
                self.radar_bubbles.focus.as_ref(),
                self.radar_bubbles.layout_progress,
            );
            let mut min_x = f32::INFINITY;
            let mut min_y = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            for file in map
                .nodes
                .iter()
                .filter(|file| !file.directory && focus.files.contains(&file.path))
            {
                let (x, y) = focus.target(file);
                min_x = min_x.min(x - file.radius);
                min_y = min_y.min(y - file.radius);
                max_x = max_x.max(x + file.radius);
                max_y = max_y.max(y + file.radius);
            }
            let span_x = (max_x - min_x).max(node.radius * 2.);
            let span_y = (max_y - min_y).max(node.radius * 2.);
            let scale = ((width - 144.).max(100.) / span_x)
                .min((height - 144.).max(100.) / span_y)
                .min(25. / node.radius);
            let zoom = (scale / fit).clamp(0.2, 100.);
            let middle = ((min_x + max_x) * 0.5, (min_y + max_y) * 0.5);
            (
                zoom,
                (-middle.0 * fit * zoom, -middle.1 * fit * zoom),
                Some(focus),
            )
        };
        self.radar_bubbles.selected = if node.directory { None } else { Some(path) };
        self.radar_bubbles.hovered = None;
        self.radar_bubbles.focus = focus;
        self.radar_bubbles.layout_progress = if self.radar_bubbles.focus.is_some() {
            0.
        } else {
            1.
        };
        self.radar_bubbles.layout_epoch = self.radar_bubbles.layout_epoch.wrapping_add(1);
        let layout_epoch = self.radar_bubbles.layout_epoch;
        if self.radar_bubbles.focus.is_some() {
            cx.spawn(async move |this, cx| {
                for step in 1..=24 {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    if let Some(this) = this.upgrade() {
                        let _ = this.update(cx, |this, cx| {
                            if this.radar_bubbles.layout_epoch == layout_epoch {
                                let t = step as f32 / 24.;
                                this.radar_bubbles.layout_progress = t * t * (3. - 2. * t);
                                cx.notify();
                            }
                        });
                    }
                }
            })
            .detach();
        }
        self.radar_bubbles.reveal = 0.;
        self.radar_bubbles.reveal_epoch = self.radar_bubbles.reveal_epoch.wrapping_add(1);
        let reveal_epoch = self.radar_bubbles.reveal_epoch;
        let stages = self
            .radar_bubbles
            .focus
            .as_ref()
            .map_or(0, |focus| focus.stages);
        if stages > 0 {
            let frames = ((stages * 8).clamp(36, 90)) as u32;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(280))
                    .await;
                for step in 1..=frames {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    if let Some(this) = this.upgrade() {
                        let _ = this.update(cx, |this, cx| {
                            if this.radar_bubbles.reveal_epoch == reveal_epoch {
                                this.radar_bubbles.reveal =
                                    stages as f32 * step as f32 / frames as f32;
                                cx.notify();
                            }
                        });
                    }
                }
            })
            .detach();
        }
        self.radar_move_camera(target_zoom, target_pan, cx);
    }

    fn radar_select_file(&mut self, path: String, cx: &mut Context<Self>) {
        if self.radar_bubbles.selected.as_deref() != Some(path.as_str()) {
            if let Some(previous) = self.radar_bubbles.selected.clone() {
                if self.radar_bubbles.history.len() == 32 {
                    self.radar_bubbles.history.remove(0);
                }
                self.radar_bubbles
                    .history
                    .push((previous, self.radar_bubbles.show_all_dependencies));
            }
            self.radar_bubbles.show_all_dependencies = false;
        }
        self.radar_focus(path, cx);
    }

    fn radar_back(&mut self, cx: &mut Context<Self>) {
        while let Some((path, show_all)) = self.radar_bubbles.history.pop() {
            let available = self.radar_bubbles.current_map().is_some_and(|map| {
                map.nodes
                    .iter()
                    .any(|node| !node.directory && node.path == path)
            });
            if available {
                self.radar_bubbles.show_all_dependencies = show_all;
                self.radar_focus(path, cx);
                return;
            }
        }
        cx.notify();
    }

    fn radar_local_point(&self, position: gpui::Point<Pixels>) -> (f32, f32) {
        let (x, y, _, _) = *self.radar_bubbles.viewport.lock().unwrap();
        (f32::from(position.x) - x, f32::from(position.y) - y)
    }

    fn radar_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, _: &mut Context<Self>) {
        self.radar_bubbles.camera_epoch = self.radar_bubbles.camera_epoch.wrapping_add(1);
        self.radar_bubbles.drag_start =
            Some((f32::from(event.position.x), f32::from(event.position.y)));
        self.radar_bubbles.drag_pan = self.radar_bubbles.pan;
        self.radar_bubbles.drag_moved = false;
    }

    fn radar_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((x, y)) = self.radar_bubbles.drag_start {
            let dx = f32::from(event.position.x) - x;
            let dy = f32::from(event.position.y) - y;
            if dx.abs() + dy.abs() > 3. {
                self.radar_bubbles.drag_moved = true;
            }
            if self.radar_bubbles.drag_moved {
                self.radar_bubbles.pan = (
                    self.radar_bubbles.drag_pan.0 + dx,
                    self.radar_bubbles.drag_pan.1 + dy,
                );
                cx.notify();
            }
            return;
        }
        let (x, y) = self.radar_local_point(event.position);
        let hit = self.radar_bubbles.hit(x, y);
        let hovered = hit.as_ref().map(|node| node.path.clone());
        if hovered != self.radar_bubbles.hovered {
            self.radar_bubbles.hovered = hovered;
            self.radar_bubbles.hover_position = (x, y);
            cx.notify();
        }
    }

    fn radar_mouse_leave(&mut self, hovered: &bool, _: &mut Window, cx: &mut Context<Self>) {
        if !hovered {
            self.radar_bubbles.hovered = None;
            cx.notify();
        }
    }

    fn radar_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.radar_bubbles.drag_start = None;
        if self.radar_bubbles.drag_moved {
            return;
        }
        let (x, y) = self.radar_local_point(event.position);
        let target = self
            .radar_bubbles
            .hit(x, y)
            .map(|bubble| (bubble.path.clone(), bubble.directory));
        if let Some((path, directory)) = target {
            if directory {
                self.radar_bubbles.show_all_dependencies = false;
                self.radar_focus(path, cx);
            } else {
                self.radar_select_file(path.clone(), cx);
                self.radar_bubbles.pulse = 1.;
                self.radar_bubbles.pulse_epoch = self.radar_bubbles.pulse_epoch.wrapping_add(1);
                let epoch = self.radar_bubbles.pulse_epoch;
                cx.spawn(async move |this, cx| {
                    for step in 1..=12 {
                        cx.background_executor()
                            .timer(Duration::from_millis(16))
                            .await;
                        if let Some(this) = this.upgrade() {
                            let _ = this.update(cx, |this, cx| {
                                if this.radar_bubbles.pulse_epoch == epoch {
                                    this.radar_bubbles.pulse = 1. - step as f32 / 12.;
                                    cx.notify();
                                }
                            });
                        }
                    }
                })
                .detach();
                if event.click_count > 1 {
                    self.choose_sidebar_file(path, true, cx);
                }
            }
        } else {
            self.radar_bubbles.selected = None;
            self.radar_bubbles.history.clear();
            self.radar_bubbles.show_all_dependencies = false;
            self.radar_bubbles.focus = None;
            self.radar_bubbles.layout_epoch = self.radar_bubbles.layout_epoch.wrapping_add(1);
            self.radar_bubbles.reveal_epoch = self.radar_bubbles.reveal_epoch.wrapping_add(1);
            self.radar_move_camera(1., (0., 0.), cx);
        }
        cx.notify();
    }

    fn radar_scroll(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.radar_bubbles.camera_epoch = self.radar_bubbles.camera_epoch.wrapping_add(1);
        let delta = event.delta.pixel_delta(px(20.));
        let dy = f32::from(delta.y);
        if event.modifiers.shift {
            self.radar_bubbles.pan.0 += f32::from(delta.x) + dy;
            self.radar_bubbles.pan.1 += f32::from(delta.y);
        } else {
            let (x, y) = self.radar_local_point(event.position);
            let (_, _, width, height) = *self.radar_bubbles.viewport.lock().unwrap();
            let factor = (dy * 0.0015).exp().clamp(0.65, 1.5);
            let before = self.radar_bubbles.zoom;
            self.radar_bubbles.zoom = (before * factor).clamp(0.2, 100.);
            let ratio = self.radar_bubbles.zoom / before;
            self.radar_bubbles.pan.0 =
                (x - width * 0.5) * (1. - ratio) + self.radar_bubbles.pan.0 * ratio;
            self.radar_bubbles.pan.1 =
                (y - height * 0.5) * (1. - ratio) + self.radar_bubbles.pan.1 * ratio;
        }
        cx.notify();
    }

    pub(super) fn radar_canvas(&self, cx: &mut Context<Self>) -> AnyElement {
        let map = self.radar_bubbles.display_map();
        let selected = self.radar_bubbles.selected.clone();
        let hovered = self.radar_bubbles.hovered.clone();
        let focus = self.radar_bubbles.focus.clone();
        let layout_progress = self.radar_bubbles.layout_progress;
        let all_mode = self.radar_bubbles.show_all_dependencies;
        let changed_only = self.radar_bubbles.show_changed_only;
        let reveal = self.radar_bubbles.reveal;
        let pulse = self.radar_bubbles.pulse;
        let zoom = self.radar_bubbles.zoom;
        let pan = self.radar_bubbles.pan;
        let viewport = self.radar_bubbles.viewport.clone();
        let mut content = div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .min_w(px(0.))
            .flex()
            .flex_col()
            .bg(rgb(0x1e2024));
        let file_count = self.radar_bubbles.map.as_ref().map_or(0, |map| map.files);
        let changed = self.radar_bubbles.map.as_ref().map_or(0, |map| map.changed);
        content = content.child(
            div()
                .h(px(40.))
                .flex_none()
                .px_4()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(rgb(0x383b42))
                .text_size(px(11.))
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .items_center()
                        .text_color(rgb(TEXT))
                        .child("Radar")
                        .child(format!("{file_count} files · {changed} changed")),
                )
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .items_center()
                        .text_color(rgb(MUTED))
                        .when(!self.radar_bubbles.history.is_empty(), |element| {
                            element.child(
                                div()
                                    .id("radar-back")
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgb(0x465773))
                                    .text_color(rgb(TEXT))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| this.radar_back(cx)))
                                    .child("← Back"),
                            )
                        })
                        .child(
                            div()
                                .id("radar-view-menu-toggle")
                                .h(px(24.))
                                .px_2()
                                .flex()
                                .items_center()
                                .gap_1()
                                .rounded_sm()
                                .bg(rgb(0x343942))
                                .text_color(rgb(TEXT))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.radar_bubbles.view_menu_open =
                                        !this.radar_bubbles.view_menu_open;
                                    cx.notify();
                                }))
                                .child("View")
                                .child(
                                    svg()
                                        .path("icons/chevron_down.svg")
                                        .size(px(12.))
                                        .flex_none()
                                        .text_color(rgb(TEXT)),
                                ),
                        )
                        .child(
                            div()
                                .id("radar-reset-view")
                                .size(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .bg(rgb(0x343942))
                                .text_color(rgb(TEXT))
                                .cursor_pointer()
                                .tooltip(|_, cx| {
                                    cx.new(|_| view::ReviewTooltip("Fit view".into())).into()
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(path) = this.radar_bubbles.selected.clone() {
                                        this.radar_focus(path, cx);
                                    } else {
                                        this.radar_move_camera(1., (0., 0.), cx);
                                    }
                                }))
                                .child(
                                    svg()
                                        .path("icons/fit_view.svg")
                                        .size(px(16.))
                                        .text_color(rgb(TEXT)),
                                ),
                        ),
                ),
        );
        if let Some(map) = map.filter(|map| !map.nodes.is_empty()) {
            let paint_map = map.clone();
            let paint_focus = focus.clone();
            let paint_selected = selected.clone();
            let paint_hovered = hovered.clone();
            let canvas = canvas(
                move |bounds, _, _| {
                    *viewport.lock().unwrap() = (
                        f32::from(bounds.origin.x),
                        f32::from(bounds.origin.y),
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height),
                    );
                },
                move |bounds, _, window, _| {
                    let width = f32::from(bounds.size.width);
                    let height = f32::from(bounds.size.height);
                    let fit = (width / (2. * paint_map.radius + 48.))
                        .min(height / (2. * paint_map.radius + 48.));
                    let scale = fit * zoom;
                    let center = (
                        f32::from(bounds.origin.x) + width * 0.5 + pan.0,
                        f32::from(bounds.origin.y) + height * 0.5 + pan.1,
                    );
                    if let Some(focus) = &paint_focus {
                        let files = paint_map
                            .nodes
                            .iter()
                            .filter(|node| !node.directory && focus.files.contains(&node.path))
                            .map(|node| (node.path.as_str(), node))
                            .collect::<HashMap<_, _>>();
                        for (from_path, to_path, stage) in &focus.edges {
                            let progress = (reveal - *stage as f32).clamp(0., 1.);
                            if progress <= 0. {
                                continue;
                            }
                            let (Some(from), Some(to)) =
                                (files.get(from_path.as_str()), files.get(to_path.as_str()))
                            else {
                                continue;
                            };
                            if changed_only && (!from.contains_changes || !to.contains_changes) {
                                continue;
                            }
                            let (from_x, from_y) = focus.position(from, layout_progress);
                            let (to_x, to_y) = focus.position(to, layout_progress);
                            let dx = (to_x - from_x) * scale;
                            let dy = (to_y - from_y) * scale;
                            let length = dx.hypot(dy);
                            let from_radius = radar::bubbles::focus_radius(
                                from,
                                scale,
                                paint_selected.as_deref() == Some(from.path.as_str()),
                            );
                            let to_radius = radar::bubbles::focus_radius(
                                to,
                                scale,
                                paint_selected.as_deref() == Some(to.path.as_str()),
                            );
                            if length <= from_radius + to_radius + 10. {
                                continue;
                            }
                            let (ux, uy) = (dx / length, dy / length);
                            let start = (
                                center.0 + from_x * scale + ux * (from_radius + 3.),
                                center.1 + from_y * scale + uy * (from_radius + 3.),
                            );
                            let end = (
                                center.0 + to_x * scale - ux * (to_radius + 7.),
                                center.1 + to_y * scale - uy * (to_radius + 7.),
                            );
                            let bend = (length * 0.075).min(25.);
                            let control = (
                                (start.0 + end.0) * 0.5 - uy * bend,
                                (start.1 + end.1) * 0.5 + ux * bend,
                            );
                            let partial_control = (
                                start.0 + (control.0 - start.0) * progress,
                                start.1 + (control.1 - start.1) * progress,
                            );
                            let second = (
                                control.0 + (end.0 - control.0) * progress,
                                control.1 + (end.1 - control.1) * progress,
                            );
                            let tip = (
                                partial_control.0 + (second.0 - partial_control.0) * progress,
                                partial_control.1 + (second.1 - partial_control.1) * progress,
                            );
                            let mut route =
                                PathBuilder::stroke(px(1.6)).dash_array(&[px(2.), px(5.)]);
                            route.move_to(point(px(start.0), px(start.1)));
                            route.curve_to(
                                point(px(tip.0), px(tip.1)),
                                point(px(partial_control.0), px(partial_control.1)),
                            );
                            if let Ok(route) = route.build() {
                                window.paint_path(route, theme::rgba(0xe8edfbdf));
                            }
                            if progress >= 1. {
                                let tangent = (end.0 - control.0, end.1 - control.1);
                                let tangent_length = tangent.0.hypot(tangent.1).max(1.);
                                let (tx, ty) =
                                    (tangent.0 / tangent_length, tangent.1 / tangent_length);
                                let mut arrow = PathBuilder::stroke(px(1.5));
                                arrow.move_to(point(
                                    px(end.0 - tx * 5. - ty * 3.),
                                    px(end.1 - ty * 5. + tx * 3.),
                                ));
                                arrow.line_to(point(px(end.0), px(end.1)));
                                arrow.line_to(point(
                                    px(end.0 - tx * 5. + ty * 3.),
                                    px(end.1 - ty * 5. - tx * 3.),
                                ));
                                if let Ok(arrow) = arrow.build() {
                                    window.paint_path(arrow, theme::rgba(0xe8edfbdf));
                                }
                            }
                        }
                    }
                    let mut index = if changed_only && paint_focus.is_some() {
                        paint_map.nodes.len()
                    } else {
                        0
                    };
                    while index < paint_map.nodes.len() {
                        let node = &paint_map.nodes[index];
                        if changed_only && !node.contains_changes {
                            index = if node.directory {
                                node.subtree_end
                            } else {
                                index + 1
                            };
                            continue;
                        }
                        if paint_focus.as_ref().is_some_and(|focus| {
                            !node.directory && focus.files.contains(&node.path)
                        }) {
                            index += 1;
                            continue;
                        }
                        let x = center.0 + node.x * scale;
                        let y = center.1 + node.y * scale;
                        let r = node.radius * scale;
                        if x + r < f32::from(bounds.origin.x)
                            || y + r < f32::from(bounds.origin.y)
                            || x - r > f32::from(bounds.origin.x + bounds.size.width)
                            || y - r > f32::from(bounds.origin.y + bounds.size.height)
                        {
                            index = if node.directory {
                                node.subtree_end
                            } else {
                                index + 1
                            };
                            continue;
                        }
                        if node.directory && r < 1.2 {
                            index = node.subtree_end;
                            continue;
                        }
                        if r >= 1.2 {
                            let mut fill_color = if node.directory {
                                0x2b2e34
                            } else {
                                bubble_color(node.change)
                            };
                            let highlighted = paint_selected.as_deref() == Some(node.path.as_str());
                            let hovered = paint_hovered.as_deref() == Some(node.path.as_str());
                            let hovered_file = hovered && !node.directory;
                            if node.directory && hovered {
                                fill_color = 0x25282e;
                            } else if highlighted {
                                fill_color = 0x555b64;
                            }
                            let related = paint_focus.as_ref().is_some_and(|focus| {
                                !node.directory && focus.files.contains(&node.path)
                            });
                            let dimmed = paint_focus.is_some() && !related;
                            let bounds = Bounds {
                                origin: point(px(x - r), px(y - r)),
                                size: size(px(2. * r), px(2. * r)),
                            };
                            window.paint_quad(quad(
                                bounds,
                                px(r),
                                theme::rgba((fill_color << 8) | if dimmed { 0x12 } else { 0xff }),
                                px(if hovered_file {
                                    1.8
                                } else if highlighted {
                                    1.4 + pulse
                                } else if related {
                                    1.4
                                } else if node.directory {
                                    1.
                                } else {
                                    0.
                                }),
                                theme::rgba(if hovered_file {
                                    0xf2f4f6ee
                                } else if highlighted {
                                    0xb5bdca99
                                } else if related {
                                    0xe8edfb99
                                } else if dimmed {
                                    0x393d4512
                                } else {
                                    0x393d45ff
                                }),
                                BorderStyle::Solid,
                            ));
                        }
                        index += 1;
                    }
                    if let Some(focus) = &paint_focus {
                        for node in paint_map.nodes.iter().filter(|node| {
                            !node.directory
                                && focus.files.contains(&node.path)
                                && (!changed_only || node.contains_changes)
                        }) {
                            let (world_x, world_y) = focus.position(node, layout_progress);
                            let highlighted = paint_selected.as_deref() == Some(node.path.as_str());
                            let hovered = paint_hovered.as_deref() == Some(node.path.as_str());
                            let x = center.0 + world_x * scale;
                            let y = center.1 + world_y * scale;
                            let r = radar::bubbles::focus_radius(node, scale, highlighted);
                            if r < 1.2
                                || x + r < f32::from(bounds.origin.x)
                                || y + r < f32::from(bounds.origin.y)
                                || x - r > f32::from(bounds.origin.x + bounds.size.width)
                                || y - r > f32::from(bounds.origin.y + bounds.size.height)
                            {
                                continue;
                            }
                            window.paint_quad(quad(
                                Bounds {
                                    origin: point(px(x - r), px(y - r)),
                                    size: size(px(2. * r), px(2. * r)),
                                },
                                px(r),
                                rgb(if highlighted {
                                    0x555b64
                                } else {
                                    bubble_color(node.change)
                                }),
                                px(if hovered {
                                    1.8
                                } else if highlighted {
                                    1.4 + pulse
                                } else {
                                    1.4
                                }),
                                theme::rgba(if hovered {
                                    0xf2f4f6ee
                                } else if highlighted {
                                    0xb5bdca99
                                } else {
                                    0xe8edfb66
                                }),
                                BorderStyle::Solid,
                            ));
                        }
                    }
                },
            )
            .size_full();
            let mut surface = div()
                .id("radar-map")
                .relative()
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(Self::radar_mouse_down))
                .on_mouse_move(cx.listener(Self::radar_mouse_move))
                .on_hover(cx.listener(Self::radar_mouse_leave))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::radar_mouse_up))
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| {
                        this.radar_bubbles.drag_start = None;
                    }),
                )
                .on_scroll_wheel(cx.listener(Self::radar_scroll))
                .child(canvas);
            // A limited set of labels keeps the map legible and the element
            // tree small even for repositories with thousands of folders.
            let (_, _, width, height) = *self.radar_bubbles.viewport.lock().unwrap();
            let fit = (width / (2. * map.radius + 48.)).min(height / (2. * map.radius + 48.));
            let scale = fit * zoom;
            for (index, (node, x, y)) in map
                .nodes
                .iter()
                .filter(|n| n.directory)
                .filter(|_| focus.is_none())
                .filter(|n| !changed_only || n.contains_changes)
                .filter(|n| n.radius * scale > 30.)
                .filter_map(|node| {
                    let x = width * 0.5 + pan.0 + node.x * scale;
                    let y = height * 0.5 + pan.1 + (node.y - node.radius) * scale - 17.;
                    (x >= 0. && x <= width && y >= 0. && y <= height).then_some((node, x, y))
                })
                .take(160)
                .enumerate()
            {
                let label_width = if changed_only { 220. } else { 88. };
                surface = surface.child(
                    div()
                        .id(("radar-directory-label", index))
                        .absolute()
                        .left(px(x - label_width * 0.5))
                        .top(px(y))
                        .w(px(label_width))
                        .text_center()
                        .text_size(px(11.))
                        .text_color(rgb(0xb5bac3))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .tooltip({
                            let label = node.label.clone();
                            move |_, cx| cx.new(|_| view::ReviewTooltip(label.clone())).into()
                        })
                        .child(div().truncate().child(node.label.clone())),
                );
            }
            if let Some(focus) = &focus {
                let mut labeled = map
                    .nodes
                    .iter()
                    .filter(|node| {
                        !node.directory
                            && focus.files.contains(&node.path)
                            && (!changed_only || node.contains_changes)
                    })
                    .collect::<Vec<_>>();
                labeled.sort_by(|a, b| {
                    (selected.as_deref() != Some(a.path.as_str()))
                        .cmp(&(selected.as_deref() != Some(b.path.as_str())))
                        .then(b.importance.total_cmp(&a.importance))
                });
                for (index, node) in labeled.into_iter().take(48).enumerate() {
                    let (world_x, world_y) = focus.position(node, layout_progress);
                    let x = width * 0.5 + pan.0 + world_x * scale;
                    let radius = radar::bubbles::focus_radius(
                        node,
                        scale,
                        selected.as_deref() == Some(node.path.as_str()),
                    );
                    let y = height * 0.5 + pan.1 + world_y * scale - radius - 22.;
                    if x < 0. || x > width || y < 0. || y > height {
                        continue;
                    }
                    let is_selected = selected.as_deref() == Some(node.path.as_str());
                    surface = surface.child(
                        div()
                            .id(("radar-focus-label", index))
                            .absolute()
                            .left(px(x - 72.))
                            .top(px(y))
                            .w(px(144.))
                            .text_center()
                            .text_size(px(if is_selected { 11. } else { 10. }))
                            .text_color(theme::rgba(if is_selected {
                                0xe8edfbff
                            } else {
                                0xcbd3e5cc
                            }))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .tooltip({
                                let label = node.label.clone();
                                move |_, cx| cx.new(|_| view::ReviewTooltip(label.clone())).into()
                            })
                            .child(div().truncate().child(node.label.clone())),
                    );
                }
            }
            if let Some(path) = &self.radar_bubbles.selected {
                if let Some(node) = map.nodes.iter().find(|n| !n.directory && &n.path == path) {
                    let linked_changes = map
                        .nodes
                        .iter()
                        .filter(|candidate| {
                            !candidate.directory
                                && candidate.change != radar::Change::Unchanged
                                && candidate.path != *path
                                && focus
                                    .as_ref()
                                    .is_some_and(|focus| focus.files.contains(&candidate.path))
                        })
                        .count();
                    let summary = if all_mode {
                        format!(
                            "{} connected files · {} imports",
                            focus.as_ref().map_or(1, |focus| focus.files.len()),
                            focus.as_ref().map_or(0, |focus| focus.edges.len())
                        )
                    } else if linked_changes == 0 {
                        "No dependency path to another changed file".to_string()
                    } else {
                        format!(
                            "{linked_changes} changed files linked · {} files on paths",
                            focus.as_ref().map_or(1, |focus| focus.files.len())
                        )
                    };
                    let path = path.clone();
                    surface = surface.child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .bottom(px(16.))
                            .p_3()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0x4d5665))
                            .bg(rgb(0x30343d))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_size(px(11.))
                            .text_color(rgb(TEXT))
                            .child(path.clone())
                            .child(summary)
                            .child(format!(
                                "≈{} lines · {} direct dependents · importance {:.1}",
                                node.lines, node.dependents, node.importance
                            ))
                            .child(
                                div()
                                    .id("radar-open-file")
                                    .mt_1()
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .bg(rgb(0x465773))
                                    .cursor_pointer()
                                    .on_mouse_up(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .child("Open file")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.choose_sidebar_file(path.clone(), true, cx)
                                    })),
                            ),
                    );
                }
            }
            if let Some(path) = &self.radar_bubbles.hovered {
                if let Some(node) = map.nodes.iter().find(|n| &n.path == path) {
                    let (x, y) = self.radar_bubbles.hover_position;
                    let x = (x + 14.).min((width - 230.).max(0.));
                    let y = (y + 14.).min((height - 42.).max(0.));
                    surface = surface.child(
                        div()
                            .absolute()
                            .left(px(x))
                            .top(px(y))
                            .max_w(px(230.))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(rgb(0x515763))
                            .bg(rgb(0x343941))
                            .text_size(px(10.))
                            .text_color(rgb(TEXT))
                            .child(if node.directory {
                                format!("{path}/")
                            } else {
                                path.clone()
                            }),
                    );
                }
            }
            surface = surface.child(
                div()
                    .absolute()
                    .right(px(16.))
                    .bottom(px(16.))
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child("Drag to move · Double click to open"),
            );
            content = content.child(surface);
        } else {
            content = content.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(MUTED))
                    .text_size(px(12.))
                    .child(if self.radar_bubbles.loading {
                        "Mapping repository…"
                    } else {
                        "No files to display"
                    }),
            );
        }
        if self.radar_bubbles.view_menu_open {
            content = content
                .child(
                    div()
                        .id("radar-view-menu-dismiss")
                        .absolute()
                        .size_full()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.radar_bubbles.view_menu_open = false;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("radar-view-menu")
                        .absolute()
                        .top(px(36.))
                        .right(px(52.))
                        .w(px(164.))
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(0x303641))
                        .shadow_lg()
                        .child(
                            div()
                                .h(px(20.))
                                .px_2()
                                .text_size(px(11.))
                                .text_color(rgb(MUTED))
                                .child("View"),
                        )
                        .children([false, true].into_iter().map(|option| {
                            div()
                                .id(if option {
                                    "radar-only-changes"
                                } else {
                                    "radar-all-files"
                                })
                                .mx_1()
                                .h(px(26.))
                                .px_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .rounded_sm()
                                .cursor_pointer()
                                .text_size(px(11.))
                                .text_color(rgb(TEXT))
                                .hover(|element| element.bg(rgb(0x3b4350)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.radar_set_changed_only(option, cx)
                                }))
                                .child(
                                    div()
                                        .w(px(16.))
                                        .text_color(rgb(BLUE))
                                        .child(if changed_only == option { "✓" } else { "" }),
                                )
                                .child(if option { "Only changes" } else { "All files" })
                        })),
                );
        }
        content.into_any_element()
    }
}
