//! Application state management

use std::sync::Arc;
use std::time::Duration;

use crate::analysis::WaveformGenerator;
use crate::audio::{AudioBuffer, AudioChannelMode, AudioPlayer};

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

/// Named marker on the audio timeline.
#[derive(Debug, Clone)]
pub struct TimelineTag {
    /// Stable identifier used for editing and hit-testing.
    pub id: u64,
    /// Tag position in seconds.
    pub time: f64,
    /// User-visible name.
    pub name: String,
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
    /// Selected source-channel playback mode.
    pub channel_mode: AudioChannelMode,
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
    /// Timeline markers shown on the waveform and seek bar.
    pub timeline_tags: Vec<TimelineTag>,
    /// Incrementing id for new timeline tags.
    pub next_timeline_tag_id: u64,
    /// Tag currently being renamed.
    pub editing_timeline_tag_id: Option<u64>,
    /// Inline editor text for the active tag rename.
    pub timeline_tag_editor_text: String,
    /// Request focus for the inline tag editor on the next frame.
    pub timeline_tag_editor_needs_focus: bool,
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
            channel_mode: AudioChannelMode::Stereo,
            loop_region: None,
            selecting_loop: false,
            loop_selection_start: None,
            error: None,
            zoom: 1.0,
            scroll_offset: 0.0,
            timeline_tags: Vec::new(),
            next_timeline_tag_id: 1,
            editing_timeline_tag_id: None,
            timeline_tag_editor_text: String::new(),
            timeline_tag_editor_needs_focus: false,
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

    /// Set source-channel playback mode.
    pub fn set_channel_mode(&mut self, mode: AudioChannelMode) {
        self.channel_mode = mode;
        if let Some(player) = &self.audio_player {
            player.set_channel_mode(self.channel_mode);
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

    /// Add a new timeline tag at the given position and return its id.
    pub fn add_timeline_tag(&mut self, position: f64) -> u64 {
        let id = self.next_timeline_tag_id;
        self.next_timeline_tag_id += 1;

        let tag = TimelineTag {
            id,
            time: position.clamp(0.0, self.duration),
            name: format!("Tag {}", id),
        };

        self.timeline_tags.push(tag);
        self.timeline_tags
            .sort_by(|left, right| left.time.total_cmp(&right.time));

        id
    }

    /// Look up a tag by id.
    pub fn timeline_tag(&self, id: u64) -> Option<&TimelineTag> {
        self.timeline_tags.iter().find(|tag| tag.id == id)
    }

    /// Begin inline editing for a timeline tag.
    pub fn begin_timeline_tag_edit(&mut self, id: u64) {
        if let Some(name) = self.timeline_tag(id).map(|tag| tag.name.clone()) {
            self.editing_timeline_tag_id = Some(id);
            self.timeline_tag_editor_text = name;
            self.timeline_tag_editor_needs_focus = true;
        }
    }

    /// Finish timeline tag editing, optionally applying the current editor text.
    pub fn finish_timeline_tag_edit(&mut self, apply: bool) {
        if apply {
            if let Some(id) = self.editing_timeline_tag_id {
                if let Some(tag) = self.timeline_tags.iter_mut().find(|tag| tag.id == id) {
                    let trimmed = self.timeline_tag_editor_text.trim();
                    tag.name = if trimmed.is_empty() {
                        format_time(tag.time)
                    } else {
                        trimmed.to_string()
                    };
                }
            }
        }

        self.editing_timeline_tag_id = None;
        self.timeline_tag_editor_text.clear();
        self.timeline_tag_editor_needs_focus = false;
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
    fn channel_mode_changes_are_forwarded_to_player() {
        let mut state = AppState::new();
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 64], 2, 48_000));
        let player = Arc::new(
            crate::audio::AudioPlayer::new(buffer.clone()).expect("player should initialize"),
        );

        state.audio_buffer = Some(buffer.clone());
        state.audio_player = Some(player.clone());

        state.set_channel_mode(AudioChannelMode::Left);

        assert_eq!(state.channel_mode, AudioChannelMode::Left);
        assert_eq!(player.channel_mode(), AudioChannelMode::Left);
        assert_eq!(buffer.channel_mode(), AudioChannelMode::Left);
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

    #[test]
    fn timeline_tags_are_inserted_in_time_order() {
        let mut state = AppState::new();
        state.duration = 12.0;

        state.add_timeline_tag(8.0);
        state.add_timeline_tag(2.0);
        state.add_timeline_tag(5.0);

        let times = state
            .timeline_tags
            .iter()
            .map(|tag| tag.time)
            .collect::<Vec<_>>();

        assert_eq!(times, vec![2.0, 5.0, 8.0]);
    }

    #[test]
    fn finishing_tag_edit_uses_new_name() {
        let mut state = AppState::new();
        state.duration = 12.0;
        let tag_id = state.add_timeline_tag(3.5);

        state.begin_timeline_tag_edit(tag_id);
        state.timeline_tag_editor_text = "Verse In".to_string();
        state.finish_timeline_tag_edit(true);

        assert_eq!(
            state
                .timeline_tag(tag_id)
                .expect("tag should still exist")
                .name,
            "Verse In"
        );
        assert_eq!(state.editing_timeline_tag_id, None);
    }
}
