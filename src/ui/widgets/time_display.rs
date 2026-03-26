//! Time display widget

use crate::state::AppState;
use crate::ui::theme::Theme;
use egui::{RichText, Ui};

/// Time display widget
pub struct TimeDisplay<'a> {
    state: &'a AppState,
    theme: &'a Theme,
}

impl<'a> TimeDisplay<'a> {
    /// Create new time display
    pub fn new(state: &'a AppState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }

    /// Show the time display
    pub fn show(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Current position
            ui.label(
                RichText::new(self.state.position_string())
                    .color(self.theme.text)
                    .size(18.0)
                    .monospace(),
            );

            ui.label(RichText::new("/").color(self.theme.text_muted));

            // Total duration
            ui.label(
                RichText::new(self.state.duration_string())
                    .color(self.theme.text_secondary)
                    .size(18.0)
                    .monospace(),
            );
        });
    }
}
