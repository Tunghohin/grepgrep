//! Main window layout

use egui::{Button, CentralPanel, ComboBox, RichText, SidePanel, Slider, TopBottomPanel};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

use crate::analysis::WaveformGenerator;
use crate::audio::{AudioBuffer, AudioChannelMode, AudioDecoder, AudioPlayer};
use crate::project::{default_project_file_name, load_from_path, save_to_file, ProjectData};
use crate::state::AppState;
use crate::ui::theme::Theme;
use crate::ui::widgets::{
    LoopControl, PlaybackControls, SpeedControl, TimeDisplay, WaveformDisplay,
};

enum PendingLoad {
    Audio(String),
    Project(PathBuf),
}

/// Main application window
pub struct MainWindow {
    /// Application state
    pub state: AppState,
    /// Theme
    theme: Theme,
    /// File path input for testing
    file_path_input: String,
    /// Pending audio file or project to load (to avoid borrow issues)
    pending_load: Option<PendingLoad>,
    /// Extracted temp directory for the currently open archived project.
    open_project_tempdir: Option<TempDir>,
}

impl MainWindow {
    /// Create a new main window
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
            theme: Theme::default(),
            file_path_input: String::new(),
            pending_load: None,
            open_project_tempdir: None,
        }
    }

    /// Set initial volume (applied when audio is loaded)
    pub fn set_initial_volume(&mut self, volume: f32) {
        self.state.volume = volume.clamp(0.0, 1.0);
    }

    /// Load a file from path (public interface for CLI)
    pub fn load_file_from_path(&mut self, path: &str) {
        self.file_path_input = path.to_string();
        self.pending_load = Some(PendingLoad::Audio(path.to_string()));
    }

    /// Load an audio file and optionally restore project data.
    fn load_audio_file(
        &mut self,
        path: String,
        project_data: Option<ProjectData>,
        project_path: Option<PathBuf>,
    ) {
        if let Some(player) = &self.state.audio_player {
            player.stop();
        }

        if project_path.is_none() {
            self.open_project_tempdir = None;
        }

        match AudioDecoder::decode_file(&path) {
            Ok(decoded) => {
                let sample_rate = decoded.sample_rate;
                let channels = decoded.channels;
                let duration = decoded.duration;
                let samples: Arc<[f32]> = decoded.samples.into();

                // Create audio buffer
                let buffer = Arc::new(AudioBuffer::new(samples.clone(), channels, sample_rate));

                // Create waveform generator
                let waveform = Arc::new(WaveformGenerator::new(samples, channels, sample_rate));

                // Create audio player
                let mut player = AudioPlayer::new(buffer.clone());

                // Initialize the audio stream
                if let Err(e) = player.init_stream() {
                    self.state.error = Some(format!("Failed to initialize audio: {}", e));
                    tracing::error!("Failed to initialize audio stream: {}", e);
                    return;
                }

                // Carry current playback settings into the new player.
                player.set_volume(self.state.volume);
                player.set_speed(self.state.speed);
                player.set_channel_mode(self.state.channel_mode);

                let player = Rc::new(player);

                // Update state
                self.state.reset_project_state();
                self.state.duration = duration.as_secs_f64();
                self.state.file_path = Some(path.clone());
                self.state.project_path = project_path;
                self.state.audio_buffer = Some(buffer);
                self.state.audio_player = Some(player);
                self.state.waveform = Some(waveform);
                self.state.error = None;

                if let Some(project_data) = project_data {
                    self.state.apply_project_data(&project_data);
                }

                tracing::info!("Loaded audio file: {} ({}s)", path, self.state.duration);
            }
            Err(e) => {
                self.state.error = Some(format!("Failed to load file: {}", e));
                tracing::error!("Failed to load file: {}", e);
            }
        }
    }

    fn open_project(&mut self, project_path: PathBuf) {
        match load_from_path(&project_path) {
            Ok(loaded_project) => {
                self.open_project_tempdir = loaded_project.extracted_dir;
                let audio_path = loaded_project.audio_path.to_string_lossy().to_string();
                self.file_path_input = audio_path.clone();
                self.load_audio_file(
                    audio_path,
                    Some(loaded_project.data),
                    loaded_project.project_path,
                );
            }
            Err(error) => {
                self.state.error = Some(format!("Failed to open project: {}", error));
                tracing::error!(
                    "Failed to open project {}: {}",
                    project_path.display(),
                    error
                );
            }
        }
    }

    fn save_project(&mut self, project_path: PathBuf) {
        match save_to_file(&self.state, &project_path) {
            Ok(saved_path) => {
                self.state.project_path = Some(saved_path.clone());
                self.state.error = None;
                tracing::info!("Saved project to {}", saved_path.display());
            }
            Err(error) => {
                self.state.error = Some(format!("Failed to save project: {}", error));
                tracing::error!(
                    "Failed to save project {}: {}",
                    project_path.display(),
                    error
                );
            }
        }
    }

    fn prompt_open_project(&mut self) {
        let mut dialog = rfd::FileDialog::new();
        if let Some(project_path) = &self.state.project_path {
            if let Some(parent) = project_path.parent() {
                dialog = dialog.set_directory(parent);
            }
        } else if let Some(file_path) = self.state.file_path.as_deref().map(Path::new) {
            if let Some(parent) = file_path.parent() {
                dialog = dialog.set_directory(parent);
            }
        }

        if let Some(path) = dialog
            .add_filter("grepgrep Project", &["ggproj"])
            .pick_file()
        {
            self.pending_load = Some(PendingLoad::Project(path));
        }
    }

    fn save_project_via_dialog(&mut self) {
        if let Some(project_path) = self.state.project_path.clone() {
            self.save_project(project_path);
            return;
        }

        let Some(audio_path) = self.state.file_path.as_deref().map(Path::new) else {
            self.state.error = Some("Load an audio file before saving a project".to_string());
            return;
        };

        let default_name = default_project_file_name(audio_path);
        let mut dialog = rfd::FileDialog::new().set_file_name(&default_name);
        if let Some(parent) = audio_path.parent() {
            dialog = dialog.set_directory(parent);
        }

        if let Some(project_path) = dialog
            .add_filter("grepgrep Project", &["ggproj"])
            .save_file()
        {
            self.save_project(project_path);
        }
    }
}

impl eframe::App for MainWindow {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme
        self.theme.apply(ctx);

        // Keep the UI ticking while playback is active without burning CPU at idle.
        if self.state.is_playing() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        // Handle pending file or project load
        if let Some(pending_load) = self.pending_load.take() {
            match pending_load {
                PendingLoad::Audio(path) => self.load_audio_file(path, None, None),
                PendingLoad::Project(project_dir) => self.open_project(project_dir),
            }
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
                if ui.add(Button::new("Open")).clicked() && !self.file_path_input.is_empty() {
                    self.pending_load = Some(PendingLoad::Audio(self.file_path_input.clone()));
                }

                // Browse button
                if ui.add(Button::new("Browse...")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio Files", &["mp3", "flac", "wav", "ogg", "aac", "m4a"])
                        .pick_file()
                    {
                        self.file_path_input = path.to_string_lossy().to_string();
                        self.pending_load = Some(PendingLoad::Audio(self.file_path_input.clone()));
                    }
                }

                if ui.add(Button::new("Open Project...")).clicked() {
                    self.prompt_open_project();
                }

                let can_save_project = self.state.audio_buffer.is_some();
                if ui
                    .add_enabled(can_save_project, Button::new("Save Project..."))
                    .clicked()
                {
                    self.save_project_via_dialog();
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

                ui.add_space(10.0);

                // Channel control
                ui.collapsing("Channel", |ui| {
                    let mut channel_mode = self.state.channel_mode;
                    ComboBox::from_label("Mode")
                        .selected_text(match channel_mode {
                            AudioChannelMode::Stereo => "Stereo",
                            AudioChannelMode::Left => "Left",
                            AudioChannelMode::Right => "Right",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut channel_mode,
                                AudioChannelMode::Stereo,
                                "Stereo",
                            );
                            ui.selectable_value(&mut channel_mode, AudioChannelMode::Left, "Left");
                            ui.selectable_value(
                                &mut channel_mode,
                                AudioChannelMode::Right,
                                "Right",
                            );
                        });
                    self.state.set_channel_mode(channel_mode);

                    if self
                        .state
                        .audio_buffer
                        .as_ref()
                        .map(|buffer| buffer.channel_count() <= 1)
                        .unwrap_or(false)
                    {
                        ui.label(
                            RichText::new("Mono files sound the same in every mode.")
                                .color(text_muted)
                                .size(12.0),
                        );
                    }
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
                WaveformDisplay::new(&waveform, &mut self.state, &theme)
                    .height(ui.available_height() - 20.0)
                    .show(ui);

                // Instructions
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(
                            "Space: Play/Pause | Ctrl+S: Save Project | Ctrl+Shift+O: Open Project | Click timeline/waveform: Play from position | Drag waveform: Select loop | Ctrl+Click: Add tag | Click tag: Play | Double-click tag: Rename"
                        )
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
                            self.pending_load = Some(PendingLoad::Audio(self.file_path_input.clone()));
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
                        "- Left / right / stereo channel selection",
                    ] {
                        ui.label(RichText::new(*feature).color(text_muted).size(12.0));
                    }
                });
            }
        });

        // Handle keyboard shortcuts
        let is_playing = self.state.is_playing();
        let has_player = self.state.audio_player.is_some();
        let has_audio = self.state.audio_buffer.is_some();
        let has_loop = self.state.loop_region.is_some();

        ctx.input(|i| {
            // Space: Play/Pause
            if i.key_pressed(egui::Key::Space) && !i.modifiers.ctrl && has_player {
                if is_playing {
                    if let Some(player) = &self.state.audio_player {
                        let _ = player.pause();
                    }
                } else if let Some(player) = &self.state.audio_player {
                    let _ = player.play();
                }
            }

            // Ctrl+Shift+O: Open project
            if i.key_pressed(egui::Key::O) && i.modifiers.ctrl && i.modifiers.shift {
                self.prompt_open_project();
            }

            // Ctrl+O: Open file
            if i.key_pressed(egui::Key::O) && i.modifiers.ctrl && !i.modifiers.shift {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Audio Files", &["mp3", "flac", "wav", "ogg", "aac", "m4a"])
                    .pick_file()
                {
                    self.file_path_input = path.to_string_lossy().to_string();
                    self.pending_load = Some(PendingLoad::Audio(self.file_path_input.clone()));
                }
            }

            // Ctrl+S: Save project
            if i.key_pressed(egui::Key::S) && i.modifiers.ctrl && has_audio {
                self.save_project_via_dialog();
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
