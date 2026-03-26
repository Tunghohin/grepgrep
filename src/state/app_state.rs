//! Application state management

use std::sync::Arc;
use std::time::Duration;

use crate::analysis::WaveformGenerator;
use crate::audio::{AudioBuffer, AudioPlayer};

/// Loop region state
#[derive(Debug, Clone, Copy, Default)]
pub struct LoopRegion {
    /// Start position in seconds
    pub start: f64,
    /// End position in seconds
    pub end: f64,
    /// Whether looping is enabled
    pub enabled: bool,
}

impl LoopRegion {
    /// Create a new loop region
    pub fn new(start: f64, end: f64) -> Self {
        Self {
            start,
            end,
            enabled: true,
        }
    }

    /// Get the duration of the loop region
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }

}

/// Application state
pub struct AppState {
    /// Currently loaded file path
    pub file_path: Option<String>,
    /// Audio buffer (if loaded)
    pub audio_buffer: Option<Arc<AudioBuffer>>,
    /// Audio player (if initialized)
    pub audio_player: Option<Arc<AudioPlayer>>,
    /// Waveform generator
    pub waveform: Option<Arc<WaveformGenerator>>,
    /// Current playback position in seconds
    pub position: f64,
    /// Total duration in seconds
    pub duration: f64,
    /// Volume (0.0 to 1.0)
    pub volume: f32,
    /// Playback speed (0.1 to 2.0, 1.0 = normal)
    pub speed: f32,
    /// Loop region
    pub loop_region: Option<LoopRegion>,
    /// Whether we're currently selecting a loop region
    pub selecting_loop: bool,
    /// Loop selection start (during drag)
    pub loop_selection_start: Option<f64>,
    /// Error message to display
    pub error: Option<String>,
    /// Zoom level (samples per pixel)
    pub zoom: f32,
    /// Scroll offset in seconds
    pub scroll_offset: f64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            file_path: None,
            audio_buffer: None,
            audio_player: None,
            waveform: None,
            position: 0.0,
            duration: 0.0,
            volume: 1.0,
            speed: 1.0,
            loop_region: None,
            selecting_loop: false,
            loop_selection_start: None,
            error: None,
            zoom: 1.0,
            scroll_offset: 0.0,
        }
    }
}

impl AppState {
    /// Create a new application state
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> bool {
        self.audio_player
            .as_ref()
            .map(|p| p.is_playing())
            .unwrap_or(false)
    }

    /// Format current position as time string
    pub fn position_string(&self) -> String {
        format_time(self.position)
    }

    /// Format duration as time string
    pub fn duration_string(&self) -> String {
        format_time(self.duration)
    }

    /// Set volume (clamped to valid range)
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(player) = &self.audio_player {
            player.set_volume(self.volume);
        }
    }

    /// Set playback speed (clamped to valid range: 0.1x to 2.0x)
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(0.1, 2.0);
        if let Some(player) = &self.audio_player {
            player.set_speed(self.speed);
        }
    }

    /// Seek to position
    pub fn seek(&mut self, position: f64) {
        self.position = position.clamp(0.0, self.duration);
        if let Some(player) = &self.audio_player {
            player.seek_time(Duration::from_secs_f64(self.position));
        }
    }

    /// Set loop region
    pub fn set_loop(&mut self, start: f64, end: f64) {
        let start = start.clamp(0.0, self.duration);
        let end = end.clamp(0.0, self.duration);

        if start < end {
            self.loop_region = Some(LoopRegion::new(start, end));
            self.sync_loop_state();
        }
    }

    /// Enable or disable the selected loop region without clearing it.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        if let Some(loop_region) = &mut self.loop_region {
            loop_region.enabled = enabled;
        }

        self.sync_loop_state();
    }

    /// Toggle loop playback for the current loop region.
    pub fn toggle_loop_enabled(&mut self) {
        if let Some(loop_region) = &mut self.loop_region {
            loop_region.enabled = !loop_region.enabled;
        }

        self.sync_loop_state();
    }

    /// Synchronize the loop selection with the audio buffer.
    pub fn sync_loop_state(&self) {
        let Some(buffer) = &self.audio_buffer else {
            return;
        };

        if let Some((start_frame, end_frame)) = self.loop_frames() {
            buffer.set_loop(Some(start_frame), Some(end_frame));
        } else {
            buffer.clear_loop();
        }
    }

    /// Clear loop region
    pub fn clear_loop(&mut self) {
        self.loop_region = None;
        self.selecting_loop = false;
        self.loop_selection_start = None;
        self.sync_loop_state();
    }

    fn loop_frames(&self) -> Option<(usize, usize)> {
        let loop_region = self.loop_region?;
        if !loop_region.enabled {
            return None;
        }

        let buffer = self.audio_buffer.as_ref()?;
        let sample_rate = buffer.sample_rate() as f64;
        let start_frame = (loop_region.start * sample_rate) as usize;
        let end_frame = (loop_region.end * sample_rate) as usize;

        if start_frame < end_frame {
            Some((start_frame, end_frame))
        } else {
            None
        }
    }
}

/// Format time in seconds to MM:SS.ms format
pub fn format_time(seconds: f64) -> String {
    let total_ms = (seconds * 1000.0) as u64;
    let minutes = total_ms / 60000;
    let secs = (total_ms % 60000) / 1000;
    let ms = total_ms % 1000;

    format!("{:02}:{:02}.{:03}", minutes, secs, ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioBuffer;

    #[test]
    fn speed_changes_are_clamped_before_reaching_player() {
        let mut state = AppState::new();
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 64], 2, 48_000));
        let player = Arc::new(
            crate::audio::AudioPlayer::new(buffer.clone()).expect("player should initialize"),
        );

        state.audio_buffer = Some(buffer.clone());
        state.audio_player = Some(player.clone());

        state.set_speed(8.0);

        assert!((state.speed - 2.0).abs() < 0.001);
        assert!((player.speed() - 2.0).abs() < 0.001);
        assert!((buffer.speed() - 2.0).abs() < 0.001);
    }

    #[test]
    fn disabling_loop_clears_buffer_loop_without_removing_selection() {
        let mut state = AppState::new();
        state.duration = 10.0;
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 48_000 * 4], 1, 48_000));

        state.audio_buffer = Some(buffer.clone());
        state.set_loop(1.0, 2.0);
        assert!(state.loop_region.expect("loop region should exist").enabled);
        assert_eq!(buffer.loop_bounds(), Some((48_000, 96_000)));

        state.set_loop_enabled(false);

        let loop_region = state
            .loop_region
            .expect("loop selection should be preserved");
        assert!(!loop_region.enabled);
        assert!(!buffer.loop_enabled());
        assert_eq!(buffer.loop_bounds(), None);
    }
}
