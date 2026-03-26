//! Loop control widget

use crate::state::AppState;
use crate::ui::theme::Theme;
use egui::{Button, RichText, Ui};

/// Loop control panel
pub struct LoopControl<'a> {
    state: &'a mut AppState,
    theme: &'a Theme,
}

impl<'a> LoopControl<'a> {
    /// Create new loop control
    pub fn new(state: &'a mut AppState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }

    /// Show the loop controls
    pub fn show(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.set_min_width(200.0);

            ui.label(
                RichText::new("Loop Region")
                    .color(self.theme.text)
                    .size(14.0),
            );

            ui.add_space(5.0);

            if let Some(loop_region) = &self.state.loop_region {
                // Show loop info
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Start: {:.3}s", loop_region.start))
                            .color(self.theme.text_secondary)
                            .monospace(),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("End: {:.3}s", loop_region.end))
                            .color(self.theme.text_secondary)
                            .monospace(),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Duration: {:.3}s", loop_region.duration()))
                            .color(self.theme.accent)
                            .monospace(),
                    );
                });

                ui.add_space(10.0);

                // Enable/disable toggle
                let toggle_text = if loop_region.enabled {
                    "Disable Loop"
                } else {
                    "Enable Loop"
                };
                if ui.add(Button::new(toggle_text)).clicked() {
                    self.state.toggle_loop_enabled();
                }

                // Clear button
                if ui.add(Button::new("Clear Selection")).clicked() {
                    self.state.clear_loop();
                }
            } else {
                ui.label(RichText::new("No loop region selected").color(self.theme.text_muted));

                ui.add_space(5.0);

                ui.label(
                    RichText::new("Drag on the waveform to select a region")
                        .color(self.theme.text_muted)
                        .size(11.0),
                );
            }
        });
    }
}
