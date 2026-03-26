//! Waveform display widget for egui

use crate::analysis::waveform::WaveformGenerator;
use crate::state::{AppState, LoopRegion, TimelineTag};
use crate::ui::theme::Theme;
use egui::{Area, FontId, Id, Key, Order, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

const TAG_HIT_RADIUS: f32 = 8.0;
const TAG_TRIANGLE_SIZE: f32 = 10.0;
const TAG_TRIANGLE_SIZE_HOVERED: f32 = 14.0;

/// Waveform display widget
pub struct WaveformDisplay<'a> {
    /// Reference to the waveform generator
    waveform: &'a WaveformGenerator,
    /// Application state
    state: &'a mut AppState,
    /// Theme
    theme: &'a Theme,
    /// Height of the display
    height: f32,
}

impl<'a> WaveformDisplay<'a> {
    /// Create a new waveform display
    pub fn new(waveform: &'a WaveformGenerator, state: &'a mut AppState, theme: &'a Theme) -> Self {
        Self {
            waveform,
            state,
            theme,
            height: 200.0,
        }
    }

    /// Set the height of the display
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Show the waveform display
    pub fn show(mut self, ui: &mut Ui) -> Response {
        let available_width = ui.available_width();

        // Top area for seeking (click to play from position)
        let seek_height = 30.0;
        let seek_size = Vec2::new(available_width, seek_height);
        let (seek_response, _) = ui.allocate_painter(seek_size, Sense::click());
        let seek_rect = seek_response.rect;

        // Waveform area for loop selection and timeline tags.
        let waveform_height = self.height - seek_height - 20.0;
        let waveform_size = Vec2::new(available_width, waveform_height);
        let (response, painter) = ui.allocate_painter(waveform_size, Sense::click_and_drag());
        let rect = response.rect;
        let painter = &painter;

        let pointer_pos = response
            .interact_pointer_pos()
            .or(seek_response.interact_pointer_pos())
            .or(response.hover_pos())
            .or(seek_response.hover_pos());
        let hovered_tag_id = pointer_pos.and_then(|pos| self.hit_test_timeline_tag(pos, seek_rect, rect));

        self.draw_seek_bar(ui, seek_rect, hovered_tag_id);

        // Handle tag interactions before seek/loop interactions so Ctrl+click and double-click
        // on markers do not trigger seek or loop-clearing behavior.
        let mut consumed = self.handle_tag_interaction(&seek_response, seek_rect, hovered_tag_id, true);
        if !consumed {
            consumed = self.handle_tag_interaction(&response, rect, hovered_tag_id, false);
        }

        if !consumed {
            self.handle_seek_click(&seek_response, seek_rect);
        }

        // Handle zoom with scroll wheel
        self.handle_zoom(&response, rect);

        // Draw waveform background
        painter.rect_filled(rect, 0.0, self.theme.waveform_background);

        // Draw center line
        let center_y = rect.center().y;
        painter.line_segment(
            [
                Pos2::new(rect.left(), center_y),
                Pos2::new(rect.right(), center_y),
            ],
            Stroke::new(1.0, self.theme.waveform_center_line),
        );

        // Generate and draw waveform (with zoom/scroll)
        if self.state.duration > 0.0 {
            self.draw_waveform_zoomed(painter, rect);
        }

        // Draw loop region if set
        if let Some(loop_region) = &self.state.loop_region {
            if loop_region.enabled {
                self.draw_loop_region(painter, rect, loop_region);
            }
        }

        // Draw timeline tags over the waveform.
        self.draw_waveform_tags(painter, rect, hovered_tag_id);

        // Draw playhead
        self.draw_playhead(painter, rect);

        // Handle waveform interactions (loop selection)
        if !consumed {
            self.handle_waveform_interaction(&response, rect);
        }

        self.show_timeline_tag_editor(ui.ctx(), seek_rect, rect);

        // Scrollbar for zoomed view
        self.draw_scrollbar(ui, available_width);

        response
    }

    fn draw_seek_bar(&self, ui: &Ui, seek_rect: Rect, hovered_tag_id: Option<u64>) {
        ui.painter()
            .rect_filled(seek_rect, 0.0, self.theme.surface_dark);

        let duration = self.state.duration;
        if duration <= 0.0 {
            return;
        }

        let zoom = self.state.zoom;
        let scroll_offset = self.state.scroll_offset;
        let visible_duration = duration / zoom as f64;
        let visible_start = scroll_offset;
        let visible_end = (visible_start + visible_duration).min(duration);

        let vis_start_x = seek_rect.left() + (visible_start / duration) as f32 * seek_rect.width();
        let vis_end_x = seek_rect.left() + (visible_end / duration) as f32 * seek_rect.width();

        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(vis_start_x, seek_rect.top() + 5.0),
                Vec2::new(vis_end_x - vis_start_x, seek_rect.height() - 10.0),
            ),
            2.0,
            self.theme.surface_light,
        );

        self.draw_seek_bar_tags(ui.painter(), seek_rect, hovered_tag_id);

        let seek_x =
            seek_rect.left() + (self.state.position / duration) as f32 * seek_rect.width();
        ui.painter().line_segment(
            [
                Pos2::new(seek_x, seek_rect.top()),
                Pos2::new(seek_x, seek_rect.bottom()),
            ],
            Stroke::new(2.0, self.theme.accent),
        );

        let time_text = format_time(self.state.position);
        ui.painter().text(
            Pos2::new(seek_rect.left() + 5.0, seek_rect.center().y),
            egui::Align2::LEFT_CENTER,
            time_text,
            FontId::proportional(12.0),
            self.theme.text_secondary,
        );

        let total_time = format_time(duration);
        ui.painter().text(
            Pos2::new(seek_rect.right() - 5.0, seek_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            total_time,
            FontId::proportional(12.0),
            self.theme.text_secondary,
        );
    }

    fn draw_seek_bar_tags(&self, painter: &Painter, seek_rect: Rect, hovered_tag_id: Option<u64>) {
        for tag in &self.state.timeline_tags {
            let x = self.tag_x_in_seek_bar(seek_rect, tag.time);
            let is_hovered = Some(tag.id) == hovered_tag_id;
            let stroke = Stroke::new(
                if is_hovered { 2.0 } else { 1.0 },
                if is_hovered {
                    self.theme.accent_hover
                } else {
                    self.theme.accent
                },
            );

            painter.line_segment(
                [
                    Pos2::new(x, seek_rect.top() + 2.0),
                    Pos2::new(x, seek_rect.bottom() - 2.0),
                ],
                stroke,
            );
        }
    }

    /// Handle zoom with mouse wheel
    fn handle_zoom(&mut self, response: &Response, _rect: Rect) {
        let zoom_delta = response.ctx.input(|i| i.raw_scroll_delta.y);

        if zoom_delta.abs() > 0.0 {
            let old_zoom = self.state.zoom;
            let zoom_factor = if zoom_delta > 0.0 { 1.1 } else { 0.9 };
            let new_zoom = (old_zoom * zoom_factor as f32).clamp(1.0, 50.0);
            self.state.zoom = new_zoom;

            if new_zoom > 1.0 {
                let duration = self.state.duration;
                let visible_duration = duration / new_zoom as f64;
                let max_scroll = duration - visible_duration;
                self.state.scroll_offset = self.state.scroll_offset.min(max_scroll).max(0.0);
            } else {
                self.state.scroll_offset = 0.0;
            }
        }
    }

    /// Draw waveform with zoom support
    fn draw_waveform_zoomed(&self, painter: &Painter, rect: Rect) {
        let duration = self.state.duration;
        let zoom = self.state.zoom;
        let scroll_offset = self.state.scroll_offset;
        let visible_duration = duration / zoom as f64;
        let visible_start = scroll_offset;
        let visible_end = (visible_start + visible_duration).min(duration);

        let rect_width = rect.width() as usize;
        if let Some(level) = self.waveform.get_level(rect_width) {
            let points = &level.points;
            if points.is_empty() {
                return;
            }

            let total_points = points.len();
            let height = rect.height();
            let center_y = rect.center().y;
            let amplitude = height / 2.0 * 0.9;

            let start_ratio = visible_start / duration;
            let end_ratio = visible_end / duration;

            let start_idx = (start_ratio * total_points as f64) as usize;
            let end_idx = (end_ratio * total_points as f64) as usize;
            let num_points = (end_idx - start_idx).max(1);

            for pixel_x in 0..rect_width {
                let point_idx = start_idx + (pixel_x * num_points / rect_width).min(num_points - 1);

                if point_idx < points.len() {
                    let point = points[point_idx];
                    let x = rect.left() + pixel_x as f32;

                    let top_y = center_y - point.max * amplitude;
                    let bottom_y = center_y - point.min * amplitude;

                    painter.line_segment(
                        [Pos2::new(x, top_y), Pos2::new(x, bottom_y)],
                        self.theme.waveform_stroke(1.0),
                    );
                }
            }
        }
    }

    fn draw_waveform_tags(&self, painter: &Painter, rect: Rect, hovered_tag_id: Option<u64>) {
        for tag in &self.state.timeline_tags {
            let Some(x) = self.tag_x_in_waveform(rect, tag.time) else {
                continue;
            };

            let is_hovered =
                Some(tag.id) == hovered_tag_id || Some(tag.id) == self.state.editing_timeline_tag_id;
            let color = if is_hovered {
                self.theme.accent_hover
            } else {
                self.theme.accent
            };
            let triangle_size = if is_hovered {
                TAG_TRIANGLE_SIZE_HOVERED
            } else {
                TAG_TRIANGLE_SIZE
            };
            let line_top = rect.top() + triangle_size + 6.0;

            painter.line_segment(
                [Pos2::new(x, line_top), Pos2::new(x, rect.bottom())],
                Stroke::new(if is_hovered { 2.0 } else { 1.0 }, color),
            );

            let triangle = vec![
                Pos2::new(x - triangle_size / 2.0, rect.top() + 4.0),
                Pos2::new(x + triangle_size / 2.0, rect.top() + 4.0),
                Pos2::new(x, rect.top() + triangle_size + 4.0),
            ];
            painter.add(egui::epaint::PathShape::convex_polygon(
                triangle,
                color,
                Stroke::NONE,
            ));

            if is_hovered {
                self.draw_tag_label(painter, rect, x, line_top + 6.0, tag);
            }
        }
    }

    fn draw_tag_label(&self, painter: &Painter, rect: Rect, x: f32, top: f32, tag: &TimelineTag) {
        let label = if tag.name.trim().is_empty() {
            format_time(tag.time)
        } else {
            tag.name.clone()
        };

        let font = FontId::proportional(15.0);
        let width = label.chars().count() as f32 * 8.0 + 20.0;
        let label_rect = Rect::from_min_size(
            Pos2::new((x - width / 2.0).clamp(rect.left() + 4.0, rect.right() - width - 4.0), top),
            Vec2::new(width, 24.0),
        );

        painter.rect_filled(label_rect, 6.0, self.theme.surface);
        painter.rect_stroke(
            label_rect,
            6.0,
            Stroke::new(1.0, self.theme.accent_hover),
        );
        painter.text(
            label_rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            self.theme.text,
        );
    }

    /// Draw scrollbar for zoomed view
    fn draw_scrollbar(&mut self, ui: &mut Ui, width: f32) {
        let duration = self.state.duration;
        if duration <= 0.0 || self.state.zoom <= 1.0 {
            return;
        }

        let zoom = self.state.zoom;
        let scroll_offset = self.state.scroll_offset;
        let visible_duration = duration / zoom as f64;

        let scrollbar_height = 15.0;
        let scrollbar_rect =
            Rect::from_min_size(ui.cursor().min, Vec2::new(width, scrollbar_height));

        ui.painter()
            .rect_filled(scrollbar_rect, 0.0, self.theme.surface_dark);

        let thumb_start = (scroll_offset / duration) as f32 * width;
        let thumb_width = (visible_duration / duration) as f32 * width;
        let thumb_rect = Rect::from_min_size(
            Pos2::new(
                scrollbar_rect.left() + thumb_start,
                scrollbar_rect.top() + 2.0,
            ),
            Vec2::new(thumb_width, scrollbar_height - 4.0),
        );

        ui.painter()
            .rect_filled(thumb_rect, 2.0, self.theme.surface_light);

        let scrollbar_response = ui.allocate_rect(scrollbar_rect, Sense::click_and_drag());

        if scrollbar_response.dragged() || scrollbar_response.clicked() {
            if let Some(pos) = scrollbar_response.interact_pointer_pos() {
                let x_ratio = (pos.x - scrollbar_rect.left()) / scrollbar_rect.width();
                let new_offset = (x_ratio as f64 * duration - visible_duration / 2.0)
                    .clamp(0.0, duration - visible_duration);
                self.state.scroll_offset = new_offset;
            }
        }
    }

    fn handle_tag_interaction(
        &mut self,
        response: &Response,
        rect: Rect,
        hovered_tag_id: Option<u64>,
        full_duration_scale: bool,
    ) -> bool {
        let duration = self.state.duration;
        if duration <= 0.0 {
            return false;
        }

        let modifiers = response.ctx.input(|i| i.modifiers);

        if response.double_clicked() {
            if let Some(tag_id) = hovered_tag_id {
                if let Some(pos) = response.interact_pointer_pos() {
                    if rect.contains(pos) {
                        self.state.begin_timeline_tag_edit(tag_id);
                        return true;
                    }
                }
            }
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if rect.contains(pos) {
                    if modifiers.ctrl {
                        let time = if full_duration_scale {
                            self.seek_bar_x_to_time(rect, pos.x)
                        } else {
                            self.waveform_x_to_time(rect, pos.x)
                        };
                        self.state.add_timeline_tag(time);
                        return true;
                    }

                    if let Some(tag_id) = hovered_tag_id {
                        if let Some(tag) = self.state.timeline_tag(tag_id) {
                            self.state.seek(tag.time);
                            self.state.sync_loop_state();

                            if let Some(player) = &self.state.audio_player {
                                let _ = player.play();
                            }
                        }

                        return true;
                    }
                }
            }
        }

        false
    }

    /// Handle seek bar click - play from position
    fn handle_seek_click(&mut self, response: &Response, rect: Rect) {
        let duration = self.state.duration;
        if duration <= 0.0 {
            return;
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if rect.contains(pos) {
                    let time = self.seek_bar_x_to_time(rect, pos.x);
                    self.state.seek(time);
                    self.state.sync_loop_state();

                    if let Some(player) = &self.state.audio_player {
                        let _ = player.play();
                    }
                }
            }
        }
    }

    /// Handle waveform interactions - loop selection by dragging on waveform
    fn handle_waveform_interaction(&mut self, response: &Response, rect: Rect) {
        let duration = self.state.duration;
        if duration <= 0.0 {
            return;
        }

        if response.ctx.input(|i| i.modifiers.ctrl) {
            return;
        }

        let pointer_pos = response.interact_pointer_pos().or(response.hover_pos());

        if response.drag_started() {
            if let Some(pos) = pointer_pos {
                if rect.contains(pos) {
                    let time = self.waveform_x_to_time(rect, pos.x);
                    self.state.loop_selection_start = Some(time);
                    self.state.selecting_loop = true;
                    tracing::debug!("Loop selection started at: {}", time);
                }
            }
        }

        if response.dragged() && self.state.selecting_loop {
            if let Some(pos) = pointer_pos {
                if let Some(start_time) = self.state.loop_selection_start {
                    let current_time = self.waveform_x_to_time(rect, pos.x);
                    let loop_start = start_time.min(current_time);
                    let loop_end = start_time.max(current_time);

                    if (loop_end - loop_start) > 0.05 {
                        self.state.loop_region = Some(LoopRegion {
                            start: loop_start,
                            end: loop_end,
                            enabled: true,
                        });
                        self.state.sync_loop_state();
                    }
                }
            }
        }

        if response.drag_stopped() {
            self.state.selecting_loop = false;
            self.state.loop_selection_start = None;
        }

        if response.double_clicked() {
            self.state.clear_loop();
            return;
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if rect.contains(pos) {
                    let time = self.waveform_x_to_time(rect, pos.x);
                    self.state.seek(time);
                    self.state.sync_loop_state();

                    if let Some(player) = &self.state.audio_player {
                        let _ = player.play();
                    }
                }
            }
        }
    }

    /// Draw loop region
    fn draw_loop_region(&self, painter: &Painter, rect: Rect, loop_region: &LoopRegion) {
        let duration = self.state.duration;
        if duration <= 0.0 {
            return;
        }

        let zoom = self.state.zoom;
        let scroll_offset = self.state.scroll_offset;
        let visible_duration = duration / zoom as f64;
        let width = rect.width();

        let start_x =
            rect.left() + ((loop_region.start - scroll_offset) / visible_duration) as f32 * width;
        let end_x =
            rect.left() + ((loop_region.end - scroll_offset) / visible_duration) as f32 * width;

        if end_x < rect.left() || start_x > rect.right() {
            return;
        }

        let start_x = start_x.max(rect.left());
        let end_x = end_x.min(rect.right());

        if end_x <= start_x {
            return;
        }

        let selection_rect = Rect::from_min_size(
            Pos2::new(start_x, rect.top()),
            Vec2::new(end_x - start_x, rect.height()),
        );

        painter.rect_filled(selection_rect, 0.0, self.theme.waveform_selection);
        painter.line_segment(
            [
                Pos2::new(start_x, rect.top()),
                Pos2::new(start_x, rect.bottom()),
            ],
            self.theme.selection_stroke(),
        );
        painter.line_segment(
            [
                Pos2::new(end_x, rect.top()),
                Pos2::new(end_x, rect.bottom()),
            ],
            self.theme.selection_stroke(),
        );
    }

    /// Draw playhead
    fn draw_playhead(&self, painter: &Painter, rect: Rect) {
        let duration = self.state.duration;
        if duration <= 0.0 {
            return;
        }

        let zoom = self.state.zoom;
        let scroll_offset = self.state.scroll_offset;
        let visible_duration = duration / zoom as f64;
        let width = rect.width();
        let position = self.state.position;
        let x = rect.left() + ((position - scroll_offset) / visible_duration) as f32 * width;

        if x < rect.left() || x > rect.right() {
            return;
        }

        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0, self.theme.waveform_playhead),
        );

        let triangle_size = 8.0;
        let triangle = vec![
            Pos2::new(x - triangle_size / 2.0, rect.top()),
            Pos2::new(x + triangle_size / 2.0, rect.top()),
            Pos2::new(x, rect.top() + triangle_size),
        ];
        painter.add(egui::epaint::PathShape::convex_polygon(
            triangle,
            self.theme.waveform_playhead,
            Stroke::NONE,
        ));
    }

    fn show_timeline_tag_editor(&mut self, ctx: &egui::Context, seek_rect: Rect, waveform_rect: Rect) {
        let Some(tag_id) = self.state.editing_timeline_tag_id else {
            return;
        };

        let Some(tag) = self.state.timeline_tag(tag_id).cloned() else {
            self.state.finish_timeline_tag_edit(false);
            return;
        };

        let editor_x = self
            .tag_x_in_waveform(waveform_rect, tag.time)
            .unwrap_or_else(|| self.tag_x_in_seek_bar(seek_rect, tag.time));
        let editor_width = 180.0;
        let editor_pos = Pos2::new(
            (editor_x - editor_width / 2.0).clamp(seek_rect.left(), seek_rect.right() - editor_width),
            waveform_rect.top() + 24.0,
        );

        let mut apply = false;
        let mut cancel = false;

        Area::new(Id::new(("timeline_tag_editor", tag_id)))
            .order(Order::Foreground)
            .fixed_pos(editor_pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.state.timeline_tag_editor_text)
                            .desired_width(editor_width)
                            .hint_text("Tag name"),
                    );

                    if self.state.timeline_tag_editor_needs_focus {
                        response.request_focus();
                        self.state.timeline_tag_editor_needs_focus = false;
                    }

                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        cancel = true;
                    } else if ui.input(|i| i.key_pressed(Key::Enter)) || response.lost_focus() {
                        apply = true;
                    }
                });
            });

        if cancel {
            self.state.finish_timeline_tag_edit(false);
        } else if apply {
            self.state.finish_timeline_tag_edit(true);
        }
    }

    fn hit_test_timeline_tag(&self, pointer_pos: Pos2, seek_rect: Rect, waveform_rect: Rect) -> Option<u64> {
        let mut best_match = None;
        let mut best_distance = f32::MAX;

        for tag in &self.state.timeline_tags {
            let seek_x = self.tag_x_in_seek_bar(seek_rect, tag.time);
            let seek_distance = (pointer_pos.x - seek_x).abs();
            let on_seek_bar = seek_rect.contains(pointer_pos) && seek_distance <= TAG_HIT_RADIUS;

            let waveform_match = self.tag_x_in_waveform(waveform_rect, tag.time).and_then(|wave_x| {
                let y_min = waveform_rect.top() - 2.0;
                let y_max = waveform_rect.bottom();
                let within_y = pointer_pos.y >= y_min && pointer_pos.y <= y_max;
                let distance = (pointer_pos.x - wave_x).abs();

                if within_y && distance <= TAG_HIT_RADIUS {
                    Some(distance)
                } else {
                    None
                }
            });

            let candidate_distance = if on_seek_bar {
                Some(seek_distance)
            } else {
                waveform_match
            };

            if let Some(distance) = candidate_distance {
                if distance < best_distance {
                    best_distance = distance;
                    best_match = Some(tag.id);
                }
            }
        }

        best_match
    }

    fn tag_x_in_seek_bar(&self, rect: Rect, time: f64) -> f32 {
        rect.left() + (time / self.state.duration) as f32 * rect.width()
    }

    fn tag_x_in_waveform(&self, rect: Rect, time: f64) -> Option<f32> {
        let visible_duration = self.state.duration / self.state.zoom as f64;
        let visible_start = self.state.scroll_offset;
        let visible_end = visible_start + visible_duration;

        if time < visible_start || time > visible_end {
            return None;
        }

        Some(rect.left() + ((time - visible_start) / visible_duration) as f32 * rect.width())
    }

    fn seek_bar_x_to_time(&self, rect: Rect, x: f32) -> f64 {
        let x_ratio = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
        (x_ratio * self.state.duration).clamp(0.0, self.state.duration)
    }

    fn waveform_x_to_time(&self, rect: Rect, x: f32) -> f64 {
        let visible_duration = self.state.duration / self.state.zoom as f64;
        let x_ratio = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
        (self.state.scroll_offset + x_ratio * visible_duration).clamp(0.0, self.state.duration)
    }
}

/// Format time for display (mm:ss or ss)
fn format_time(seconds: f64) -> String {
    let total_secs = seconds as u64;
    let minutes = total_secs / 60;
    let secs = total_secs % 60;

    if minutes > 0 {
        format!("{}:{:02}", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}
