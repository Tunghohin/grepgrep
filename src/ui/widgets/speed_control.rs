//! Speed control widget

use crate::state::AppState;
use crate::ui::theme::Theme;
use egui::{Button, RichText, Slider, Ui};

/// Speed control panel
pub struct SpeedControl<'a> {
    state: &'a mut AppState,
    theme: &'a Theme,
}

impl<'a> SpeedControl<'a> {
    /// Create new speed control
    pub fn new(state: &'a mut AppState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }

    /// Show the speed controls
    pub fn show(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.set_min_width(170.0);

            ui.label(
                RichText::new("Playback Speed")
                    .color(self.theme.text)
                    .size(14.0),
            );

            ui.add_space(5.0);

            // Speed slider
            let mut speed = self.state.speed;
            ui.add_sized(
                [140.0, 0.0],
                Slider::new(&mut speed, 0.1..=2.0)
                    .text("x")
                    .show_value(true),
            );

            // Apply speed change
            if (speed - self.state.speed).abs() > 0.001 {
                self.state.set_speed(speed);
            }

            ui.add_space(5.0);

            // Quick preset buttons
            ui.horizontal_wrapped(|ui| {
                if ui.add(Button::new("0.5x")).clicked() {
                    self.state.set_speed(0.5);
                }
                if ui.add(Button::new("0.75x")).clicked() {
                    self.state.set_speed(0.75);
                }
                if ui.add(Button::new("1.0x")).clicked() {
                    self.state.set_speed(1.0);
                }
                if ui.add(Button::new("1.25x")).clicked() {
                    self.state.set_speed(1.25);
                }
                if ui.add(Button::new("1.5x")).clicked() {
                    self.state.set_speed(1.5);
                }
            });

            ui.add_space(5.0);

            // Current speed display
            ui.label(
                RichText::new(format!("Current: {:.2}x", self.state.speed))
                    .color(self.theme.accent)
                    .monospace(),
            );
        });
    }
}
