//! Playback controls widget

use crate::state::AppState;
use crate::ui::theme::Theme;
use egui::{Button, RichText, Ui};

/// Playback controls widget
pub struct PlaybackControls<'a> {
    state: &'a mut AppState,
    theme: &'a Theme,
}

impl<'a> PlaybackControls<'a> {
    /// Create new playback controls
    pub fn new(state: &'a mut AppState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }

    /// Show the playback controls
    pub fn show(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_min_width(200.0);

            // Play/Pause button
            let play_text = if self.state.is_playing() {
                "⏸"
            } else {
                "▶"
            };
            let play_button = Button::new(RichText::new(play_text).size(20.0));

            if ui.add(play_button).clicked() {
                if self.state.is_playing() {
                    // Pause
                    if let Some(player) = &self.state.audio_player {
                        let _ = player.pause();
                    }
                } else {
                    // Play
                    if let Some(player) = &self.state.audio_player {
                        let _ = player.play();
                    }
                }
            }

            // Stop button
            if ui.add(Button::new(RichText::new("⏹").size(20.0))).clicked() {
                if let Some(player) = &self.state.audio_player {
                    player.stop();
                }
            }

            // Loop toggle
            let loop_text = if self
                .state
                .loop_region
                .as_ref()
                .map(|l| l.enabled)
                .unwrap_or(false)
            {
                "🔁"
            } else {
                "↻"
            };
            let loop_color = if self
                .state
                .loop_region
                .as_ref()
                .map(|l| l.enabled)
                .unwrap_or(false)
            {
                self.theme.accent
            } else {
                self.theme.text_muted
            };

            if ui
                .add(Button::new(
                    RichText::new(loop_text).size(18.0).color(loop_color),
                ))
                .clicked()
            {
                self.state.toggle_loop_enabled();
            }
        });
    }
}
