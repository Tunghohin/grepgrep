//! Waveform display widget for egui

use crate::analysis::waveform::WaveformGenerator;
use crate::state::{AppState, LoopRegion};
use crate::ui::theme::Theme;
use egui::{Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

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

        // Draw seek bar background
        let seek_rect = seek_response.rect;
        ui.painter()
            .rect_filled(seek_rect, 0.0, self.theme.surface_dark);

        // Draw seek position indicator
        let duration = self.state.duration;
        if duration > 0.0 {
            // Calculate visible range based on zoom and scroll
            let zoom = self.state.zoom;
            let scroll_offset = self.state.scroll_offset;

            // Visible duration (zoom = 1.0 shows full waveform)
            let visible_duration = duration / zoom as f64;
            let visible_start = scroll_offset;
            let visible_end = (visible_start + visible_duration).min(duration);

            // Draw visible range in seek bar
            let vis_start_x =
                seek_rect.left() + (visible_start / duration) as f32 * seek_rect.width();
            let vis_end_x = seek_rect.left() + (visible_end / duration) as f32 * seek_rect.width();

            ui.painter().rect_filled(
                Rect::from_min_size(
                    Pos2::new(vis_start_x, seek_rect.top() + 5.0),
                    Vec2::new(vis_end_x - vis_start_x, seek_rect.height() - 10.0),
                ),
                2.0,
                self.theme.surface_light,
            );

            // Draw playhead position
            let seek_x =
                seek_rect.left() + (self.state.position / duration) as f32 * seek_rect.width();
            ui.painter().line_segment(
                [
                    Pos2::new(seek_x, seek_rect.top()),
                    Pos2::new(seek_x, seek_rect.bottom()),
                ],
                Stroke::new(2.0, self.theme.accent),
            );

            // Draw time labels
            let time_text = format_time(self.state.position);
            ui.painter().text(
                Pos2::new(seek_rect.left() + 5.0, seek_rect.center().y),
                egui::Align2::LEFT_CENTER,
                time_text,
                egui::FontId::proportional(12.0),
                self.theme.text_secondary,
            );

            let total_time = format_time(duration);
            ui.painter().text(
                Pos2::new(seek_rect.right() - 5.0, seek_rect.center().y),
                egui::Align2::RIGHT_CENTER,
                total_time,
                egui::FontId::proportional(12.0),
                self.theme.text_secondary,
            );
        }

        // Handle seek bar clicks
        self.handle_seek_click(&seek_response, seek_rect);

        // Waveform area for loop selection
        let waveform_height = self.height - seek_height - 20.0; // Reserve space for scrollbar
        let waveform_size = Vec2::new(available_width, waveform_height);

        let (response, painter) = ui.allocate_painter(waveform_size, Sense::click_and_drag());

        let rect = response.rect;
        let painter = &painter;

        // Handle zoom with scroll wheel
        self.handle_zoom(&response, rect);

        // Draw background
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
        if duration > 0.0 {
            self.draw_waveform_zoomed(painter, rect);
        }

        // Draw loop region if set
        if let Some(loop_region) = &self.state.loop_region {
            if loop_region.enabled {
                self.draw_loop_region(painter, rect, loop_region);
            }
        }

        // Draw playhead
        self.draw_playhead(painter, rect);

        // Handle waveform interactions (loop selection)
        self.handle_waveform_interaction(&response, rect);

        // Scrollbar for zoomed view
        self.draw_scrollbar(ui, available_width);

        response
    }

    /// Handle zoom with mouse wheel
    fn handle_zoom(&mut self, response: &Response, _rect: Rect) {
        let zoom_delta = response.ctx.input(|i| i.raw_scroll_delta.y);

        if zoom_delta.abs() > 0.0 {
            let old_zoom = self.state.zoom;
            // Zoom in/out (scroll up = zoom in, scroll down = zoom out)
            let zoom_factor = if zoom_delta > 0.0 { 1.1 } else { 0.9 };
            let new_zoom = (old_zoom * zoom_factor as f32).clamp(1.0, 50.0);
            self.state.zoom = new_zoom;

            // Adjust scroll offset to keep view centered
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

        // Calculate visible range
        let visible_duration = duration / zoom as f64;
        let visible_start = scroll_offset;
        let visible_end = (visible_start + visible_duration).min(duration);

        // Get waveform level for current resolution
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

            // Calculate which portion of the waveform to draw
            let start_ratio = visible_start / duration;
            let end_ratio = visible_end / duration;

            let start_idx = (start_ratio * total_points as f64) as usize;
            let end_idx = (end_ratio * total_points as f64) as usize;
            let num_points = (end_idx - start_idx).max(1);

            // Draw vertical lines for each pixel column
            for pixel_x in 0..rect_width as usize {
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

    /// Draw scrollbar for zoomed view
    fn draw_scrollbar(&mut self, ui: &mut Ui, width: f32) {
        let duration = self.state.duration;
        if duration <= 0.0 || self.state.zoom <= 1.0 {
            // No scrollbar needed when not zoomed
            return;
        }

        let zoom = self.state.zoom;
        let scroll_offset = self.state.scroll_offset;
        let visible_duration = duration / zoom as f64;

        // Scrollbar track
        let scrollbar_height = 15.0;
        let scrollbar_rect =
            Rect::from_min_size(ui.cursor().min, Vec2::new(width, scrollbar_height));

        ui.painter()
            .rect_filled(scrollbar_rect, 0.0, self.theme.surface_dark);

        // Scrollbar thumb
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

        // Make scrollbar draggable
        let scrollbar_response = ui.allocate_rect(scrollbar_rect, Sense::click_and_drag());

        if scrollbar_response.dragged() {
            if let Some(pos) = scrollbar_response.interact_pointer_pos() {
                let x_ratio = (pos.x - scrollbar_rect.left()) / scrollbar_rect.width();
                let new_offset = (x_ratio as f64 * duration - visible_duration / 2.0)
                    .clamp(0.0, duration - visible_duration);
                self.state.scroll_offset = new_offset;
            }
        }

        // Click to jump
        if scrollbar_response.clicked() {
            if let Some(pos) = scrollbar_response.interact_pointer_pos() {
                let x_ratio = (pos.x - scrollbar_rect.left()) / scrollbar_rect.width();
                let new_offset = (x_ratio as f64 * duration - visible_duration / 2.0)
                    .clamp(0.0, duration - visible_duration);
                self.state.scroll_offset = new_offset;
            }
        }
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
                    let x_ratio = (pos.x - rect.left()) / rect.width();
                    let time = (x_ratio as f64 * duration).clamp(0.0, duration);

                    // Seek to click position
                    self.state.seek(time);
                    self.state.sync_loop_state();

                    // Start playing
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

        // Get pointer position
        let pointer_pos = response.interact_pointer_pos().or(response.hover_pos());

        // Helper: convert x position to time
        let x_to_time = |x: f32| -> f64 {
            let zoom = self.state.zoom;
            let scroll_offset = self.state.scroll_offset;
            let visible_duration = duration / zoom as f64;
            let x_ratio = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
            (scroll_offset + x_ratio * visible_duration).clamp(0.0, duration)
        };

        // Drag started - record selection start time
        if response.drag_started() {
            if let Some(pos) = pointer_pos {
                if rect.contains(pos) {
                    let time = x_to_time(pos.x);
                    self.state.loop_selection_start = Some(time);
                    self.state.selecting_loop = true;
                    tracing::debug!("Loop selection started at: {}", time);
                }
            }
        }

        // During drag - update loop region in real-time
        if response.dragged() && self.state.selecting_loop {
            if let Some(pos) = pointer_pos {
                if let Some(start_time) = self.state.loop_selection_start {
                    let current_time = x_to_time(pos.x);
                    let loop_start = start_time.min(current_time);
                    let loop_end = start_time.max(current_time);

                    // Only update if selection is meaningful (> 50ms)
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

        // Drag stopped - finalize selection
        if response.drag_stopped() {
            self.state.selecting_loop = false;
            self.state.loop_selection_start = None;
        }

        // Double-click to clear loop region
        if response.double_clicked() {
            self.state.clear_loop();
        }
    }

    /// Draw loop region
    fn draw_loop_region(&self, painter: &Painter, rect: Rect, loop_region: &LoopRegion) {
        let duration = self.state.duration;
        if duration <= 0.0 {
            return;
        }

        // Account for zoom and scroll
        let zoom = self.state.zoom;
        let scroll_offset = self.state.scroll_offset;
        let visible_duration = duration / zoom as f64;

        let width = rect.width();

        // Calculate x positions based on visible range
        let start_x =
            rect.left() + ((loop_region.start - scroll_offset) / visible_duration) as f32 * width;
        let end_x =
            rect.left() + ((loop_region.end - scroll_offset) / visible_duration) as f32 * width;

        // Only draw if visible
        if end_x < rect.left() || start_x > rect.right() {
            return;
        }

        // Clamp to visible area
        let start_x = start_x.max(rect.left());
        let end_x = end_x.min(rect.right());

        if end_x <= start_x {
            return;
        }

        // Draw selection background
        let selection_rect = Rect::from_min_size(
            Pos2::new(start_x, rect.top()),
            Vec2::new(end_x - start_x, rect.height()),
        );

        painter.rect_filled(selection_rect, 0.0, self.theme.waveform_selection);

        // Draw selection borders
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

        // Account for zoom and scroll
        let zoom = self.state.zoom;
        let scroll_offset = self.state.scroll_offset;
        let visible_duration = duration / zoom as f64;

        let width = rect.width();
        let position = self.state.position;

        // Calculate x position based on visible range
        let x = rect.left() + ((position - scroll_offset) / visible_duration) as f32 * width;

        // Only draw if visible
        if x < rect.left() || x > rect.right() {
            return;
        }

        // Draw playhead line
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0, self.theme.waveform_playhead),
        );

        // Draw triangle at top
        let triangle_size = 8.0;
        let triangle = vec![
            Pos2::new(x - triangle_size / 2.0, rect.top()),
            Pos2::new(x + triangle_size / 2.0, rect.top()),
            Pos2::new(x, rect.top() + triangle_size),
        ];
        painter.add(egui::epaint::PathShape::convex_polygon(
            triangle,
            self.theme.waveform_playhead,
            Stroke::default(),
        ));
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
