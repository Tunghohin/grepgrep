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
    pub spectrogram_low: Color32,
    pub spectrogram_mid: Color32,
    pub spectrogram_high: Color32,
    pub spectrogram_grid: Color32,
    pub piano_white_key: Color32,
    pub piano_white_key_hover: Color32,
    pub piano_white_key_active: Color32,
    pub piano_black_key: Color32,
    pub piano_black_key_hover: Color32,
    pub piano_black_key_active: Color32,
    pub piano_key_border: Color32,

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
            spectrogram_low: Color32::from_rgb(18, 50, 136),
            spectrogram_mid: Color32::from_rgb(36, 196, 126),
            spectrogram_high: Color32::from_rgb(255, 232, 92),
            spectrogram_grid: Color32::from_rgba_unmultiplied(220, 240, 255, 40),
            piano_white_key: Color32::from_rgb(236, 238, 241),
            piano_white_key_hover: Color32::from_rgb(248, 250, 252),
            piano_white_key_active: Color32::from_rgb(170, 238, 228),
            piano_black_key: Color32::from_rgb(22, 28, 38),
            piano_black_key_hover: Color32::from_rgb(34, 42, 56),
            piano_black_key_active: Color32::from_rgb(0, 170, 160),
            piano_key_border: Color32::from_rgba_unmultiplied(6, 10, 16, 180),

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

    /// Convert a normalized spectrogram intensity into the heatmap palette.
    pub fn spectrogram_color(&self, intensity: f32) -> Color32 {
        let intensity = intensity.clamp(0.0, 1.0);
        if intensity <= 0.18 {
            lerp_color(
                self.waveform_background,
                self.spectrogram_low,
                intensity / 0.18,
            )
        } else if intensity <= 0.45 {
            lerp_color(
                self.spectrogram_low,
                self.spectrogram_mid,
                (intensity - 0.18) / 0.27,
            )
        } else if intensity <= 0.72 {
            lerp_color(
                self.spectrogram_mid,
                Color32::from_rgb(240, 74, 74),
                (intensity - 0.45) / 0.27,
            )
        } else {
            lerp_color(
                Color32::from_rgb(255, 170, 60),
                self.spectrogram_high,
                (intensity - 0.72) / 0.28,
            )
        }
    }

    /// Get stroke for selection outline
    pub fn selection_stroke(&self) -> Stroke {
        Stroke::new(2.0, self.accent)
    }
}

fn lerp_color(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let lerp = |start: u8, end: u8| -> u8 {
        (start as f32 + (end as f32 - start as f32) * amount).round() as u8
    };

    Color32::from_rgba_unmultiplied(
        lerp(from.r(), to.r()),
        lerp(from.g(), to.g()),
        lerp(from.b(), to.b()),
        lerp(from.a(), to.a()),
    )
}
