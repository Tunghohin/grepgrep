//! Main window layout

use egui::{Button, CentralPanel, RichText, SidePanel, Slider, TopBottomPanel};
use std::sync::Arc;

use crate::analysis::WaveformGenerator;
use crate::audio::{AudioBuffer, AudioDecoder, AudioPlayer};
use crate::state::AppState;
use crate::ui::theme::Theme;
use crate::ui::widgets::{LoopControl, PlaybackControls, SpeedControl, TimeDisplay, WaveformDisplay};

/// Main application window
pub struct MainWindow {
    /// Application state
    pub state: AppState,
    /// Theme
    theme: Theme,
    /// File path input for testing
    file_path_input: String,
    /// Pending file to load (to avoid borrow issues)
    pending_file: Option<String>,
}

impl MainWindow {
    /// Create a new main window
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
            theme: Theme::default(),
            file_path_input: String::new(),
            pending_file: None,
        }
    }

    /// Set initial volume (applied when audio is loaded)
    pub fn set_initial_volume(&mut self, volume: f32) {
        self.state.volume = volume.clamp(0.0, 1.0);
    }

    /// Load a file from path (public interface for CLI)
    pub fn load_file_from_path(&mut self, path: &str) {
        self.pending_file = Some(path.to_string());
    }

    /// Load an audio file
    fn load_file(&mut self, path: String) {
        if let Some(player) = &self.state.audio_player {
            player.stop();
        }

        match AudioDecoder::decode_file(&path) {
            Ok(decoded) => {
                // Create audio buffer
                let buffer = Arc::new(AudioBuffer::new(
                    decoded.samples.clone(),
                    decoded.channels,
                    decoded.sample_rate,
                ));

                // Create waveform generator
                let waveform = Arc::new(WaveformGenerator::new(
                    decoded.samples,
                    decoded.channels,
                    decoded.sample_rate,
                ));

                // Create audio player
                let mut player = AudioPlayer::new(buffer.clone()).unwrap_or_else(|e| {
                    panic!("Cannot create audio player: {}", e);
                });

                // Initialize the audio stream
                if let Err(e) = player.init_stream() {
                    self.state.error = Some(format!("Failed to initialize audio: {}", e));
                    tracing::error!("Failed to initialize audio stream: {}", e);
                    return;
                }

                // Carry current playback settings into the new player.
                player.set_volume(self.state.volume);
                player.set_speed(self.state.speed);

                let player = Arc::new(player);

                // Pre-generate waveform levels
                waveform.generate_multi_resolution(12800);

                // Update state
                self.state.duration = decoded.duration.as_secs_f64();
                self.state.file_path = Some(path.clone());
                self.state.audio_buffer = Some(buffer);
                self.state.audio_player = Some(player);
                self.state.waveform = Some(waveform);
                self.state.error = None;
                self.state.position = 0.0;
                self.state.loop_region = None;
                self.state.selecting_loop = false;
                self.state.loop_selection_start = None;
                self.state.zoom = 1.0;
                self.state.scroll_offset = 0.0;
                self.state.timeline_tags.clear();
                self.state.next_timeline_tag_id = 1;
                self.state.editing_timeline_tag_id = None;
                self.state.timeline_tag_editor_text.clear();
                self.state.timeline_tag_editor_needs_focus = false;

                tracing::info!("Loaded audio file: {} ({}s)", path, self.state.duration);
            }
            Err(e) => {
                self.state.error = Some(format!("Failed to load file: {}", e));
                tracing::error!("Failed to load file: {}", e);
            }
        }
    }
}

impl eframe::App for MainWindow {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme
        self.theme.apply(ctx);

        // Request continuous repaint for real-time updates
        ctx.request_repaint();

        // Handle pending file load
        if let Some(path) = self.pending_file.take() {
            self.load_file(path);
        }

        // Update playback position from player
        if let Some(player) = &self.state.audio_player {
            self.state.position = player.position_time().as_secs_f64();
        }

        // Clone data needed for UI
        let theme = self.theme.clone();
        let accent = self.theme.accent;
        let text_secondary = self.theme.text_secondary;
        let text_muted = self.theme.text_muted;
        let error_color = self.theme.error;

        // Top panel - title bar and file controls
        TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("grepgrep").color(accent));

                ui.add_space(20.0);

                // File path input
                ui.label(RichText::new("File:").color(text_secondary));
                ui.text_edit_singleline(&mut self.file_path_input);

                // Open button
                if ui.add(Button::new("Open")).clicked() {
                    if !self.file_path_input.is_empty() {
                        self.pending_file = Some(self.file_path_input.clone());
                    }
                }

                // Browse button
                if ui.add(Button::new("Browse...")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio Files", &["mp3", "flac", "wav", "ogg", "aac", "m4a"])
                        .pick_file()
                    {
                        self.file_path_input = path.to_string_lossy().to_string();
                        self.pending_file = Some(self.file_path_input.clone());
                    }
                }

                // Show error if any
                if let Some(error) = &self.state.error {
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("Error: {}", error)).color(error_color));
                }
            });
        });

        // Left side panel - controls
        SidePanel::left("control_panel")
            .default_width(250.0)
            .min_width(200.0)
            .show(ctx, |ui| {
                ui.set_min_width(200.0);

                // Loop controls
                ui.collapsing("Loop", |ui| {
                    LoopControl::new(&mut self.state, &theme).show(ui);
                });

                ui.add_space(10.0);

                // Volume control
                ui.collapsing("Volume", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Vol:").size(14.0));
                        let mut volume = self.state.volume;
                        ui.add(Slider::new(&mut volume, 0.0..=1.0).show_value(false));
                        self.state.set_volume(volume);
                        ui.label(
                            RichText::new(format!("{:.0}%", volume * 100.0)).color(text_secondary),
                        );
                    });
                });

                ui.add_space(10.0);

                // Speed control
                ui.collapsing("Speed", |ui| {
                    SpeedControl::new(&mut self.state, &theme).show(ui);
                });
            });

        // Bottom panel - playback controls
        TopBottomPanel::bottom("bottom_panel")
            .default_height(60.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Playback controls
                    PlaybackControls::new(&mut self.state, &theme).show(ui);

                    ui.add_space(20.0);

                    // Time display
                    TimeDisplay::new(&self.state, &theme).show(ui);
                });
            });

        // Central panel - waveform display
        CentralPanel::default().show(ctx, |ui| {
            // Check if we have a waveform (clone the Arc first to avoid borrow issues)
            let waveform_opt = self.state.waveform.clone();

            if let Some(waveform) = waveform_opt {
                // Waveform display
                WaveformDisplay::new(
                    &*waveform,
                    &mut self.state,
                    &theme,
                )
                .height(ui.available_height() - 20.0)
                .show(ui);

                // Instructions
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Space: Play/Pause | Ctrl+O: Open File | Drag on waveform to select loop region")
                            .color(text_muted)
                            .size(11.0)
                    );
                });
            } else {
                // No file loaded - show instructions
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);

                    ui.label(
                        RichText::new("grepgrep")
                            .color(accent)
                            .size(32.0)
                    );

                    ui.add_space(20.0);

                    ui.label(
                        RichText::new("Open an audio file to begin")
                            .color(text_secondary)
                            .size(16.0)
                    );

                    ui.add_space(20.0);

                    if ui.add(Button::new("Open File...").min_size(egui::vec2(150.0, 40.0))).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Audio Files", &["mp3", "flac", "wav", "ogg", "aac", "m4a"])
                            .pick_file() {
                            self.file_path_input = path.to_string_lossy().to_string();
                            self.pending_file = Some(self.file_path_input.clone());
                        }
                    }

                    ui.add_space(40.0);

                    ui.label(
                        RichText::new("Supported formats: MP3, FLAC, WAV, OGG, AAC, M4A")
                            .color(text_muted)
                            .size(12.0)
                    );

                    ui.add_space(20.0);

                    ui.label(
                        RichText::new("Features:")
                            .color(text_secondary)
                            .size(14.0)
                    );

                    for feature in &[
                        "- Waveform visualization with selection",
                        "- Loop region for repeated practice",
                        "- Volume control",
                    ] {
                        ui.label(RichText::new(*feature).color(text_muted).size(12.0));
                    }
                });
            }
        });

        // Handle keyboard shortcuts
        let is_playing = self.state.is_playing();
        let has_player = self.state.audio_player.is_some();
        let has_loop = self.state.loop_region.is_some();

        ctx.input(|i| {
            // Space: Play/Pause
            if i.key_pressed(egui::Key::Space) && !i.modifiers.ctrl && has_player {
                if is_playing {
                    if let Some(player) = &self.state.audio_player {
                        let _ = player.pause();
                    }
                } else {
                    if let Some(player) = &self.state.audio_player {
                        let _ = player.play();
                    }
                }
            }

            // Ctrl+O: Open file
            if i.key_pressed(egui::Key::O) && i.modifiers.ctrl {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Audio Files", &["mp3", "flac", "wav", "ogg", "aac", "m4a"])
                    .pick_file()
                {
                    self.file_path_input = path.to_string_lossy().to_string();
                    self.pending_file = Some(self.file_path_input.clone());
                }
            }

            // L: Toggle loop
            if i.key_pressed(egui::Key::L) && has_loop {
                self.state.toggle_loop_enabled();
            }

            // Escape: Stop
            if i.key_pressed(egui::Key::Escape) && has_player {
                if let Some(player) = &self.state.audio_player {
                    player.stop();
                }
            }
        });
    }
}
