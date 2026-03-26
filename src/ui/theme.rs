//! Theme configuration for the audio transcriber.

use egui::{Color32, Rounding, Stroke};

/// Theme colors inspired by modern DAW software
#[derive(Debug, Clone)]
pub struct Theme {
    // Background colors
    pub background: Color32,
    pub surface: Color32,
    pub surface_dark: Color32,
    pub surface_light: Color32,

    // Accent colors
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_active: Color32,

    // Waveform colors
    pub waveform: Color32,
    pub waveform_background: Color32,
    pub waveform_center_line: Color32,
    pub waveform_selection: Color32,
    pub waveform_playhead: Color32,

    // Text colors
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,

    // State colors
    pub error: Color32,

    // Widget styling
    pub rounding: Rounding,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // Deep dark background
            background: Color32::from_rgb(18, 18, 20),
            surface: Color32::from_rgb(28, 28, 32),
            surface_dark: Color32::from_rgb(22, 22, 26),
            surface_light: Color32::from_rgb(38, 38, 44),

            // Cyan/teal accent (modern DAW style)
            accent: Color32::from_rgb(0, 180, 180),
            accent_hover: Color32::from_rgb(40, 200, 200),
            accent_active: Color32::from_rgb(0, 220, 220),

            // Waveform - vibrant green/cyan
            waveform: Color32::from_rgb(0, 200, 160),
            waveform_background: Color32::from_rgb(24, 24, 28),
            waveform_center_line: Color32::from_rgb(60, 60, 70),
            waveform_selection: Color32::from_rgba_unmultiplied(0, 180, 180, 80),
            waveform_playhead: Color32::from_rgb(255, 100, 100),

            // Text
            text: Color32::from_rgb(240, 240, 245),
            text_secondary: Color32::from_rgb(180, 180, 190),
            text_muted: Color32::from_rgb(120, 120, 130),

            // States
            error: Color32::from_rgb(255, 90, 90),

            // Styling
            rounding: Rounding::same(6.0),
        }
    }
}

impl Theme {
    /// Apply theme to egui context
    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();

        // Override colors
        style.visuals.window_fill = self.background;
        style.visuals.panel_fill = self.surface;
        style.visuals.extreme_bg_color = self.surface_dark;

        style.visuals.widgets.noninteractive.bg_fill = self.surface;
        style.visuals.widgets.inactive.bg_fill = self.surface_light;
        style.visuals.widgets.hovered.bg_fill = self.surface_light;
        style.visuals.widgets.active.bg_fill = self.accent;

        style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.accent_hover);
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.accent_active);

        // Override text colors
        style.visuals.override_text_color = Some(self.text);

        // Button styling
        style.visuals.button_frame = true;
        style.visuals.window_rounding = self.rounding;
        style.visuals.window_shadow = egui::epaint::Shadow::NONE;

        ctx.set_style(style);
    }

    /// Get stroke for waveform outline
    pub fn waveform_stroke(&self, width: f32) -> Stroke {
        Stroke::new(width, self.waveform)
    }

    /// Get stroke for selection outline
    pub fn selection_stroke(&self) -> Stroke {
        Stroke::new(2.0, self.accent)
    }
}
