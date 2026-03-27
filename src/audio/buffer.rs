//! Audio buffer for thread-safe sample storage and pitch-preserving playback.

use cpal::Sample;
use parking_lot::Mutex;
use std::time::Duration;

/// Source-channel playback mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioChannelMode {
    /// Preserve the original channel layout.
    #[default]
    Stereo,
    /// Play only the left channel on every output channel.
    Left,
    /// Play only the right channel on every output channel.
    Right,
}

#[derive(Debug, Clone)]
struct PlaybackEngineState {
    /// Current logical source position in frames.
    display_position: f64,
    /// Playback speed multiplier.
    speed: f32,
    /// Synthesized output waiting to be consumed by the audio callback.
    pending_output: Vec<f32>,
    /// Read offset into `pending_output`.
    pending_start: usize,
    /// Predicted source frame for the next grain.
    next_grain_source_frame: f64,
    /// Whether the stretcher needs an initial grain.
    first_grain: bool,
    /// Loop enabled flag.
    loop_enabled: bool,
    /// Loop start in frames.
    loop_start_frame: Option<usize>,
    /// Loop end in frames.
    loop_end_frame: Option<usize>,
    /// Selected source-channel playback mode.
    channel_mode: AudioChannelMode,
}

impl Default for PlaybackEngineState {
    fn default() -> Self {
        Self {
            display_position: 0.0,
            speed: 1.0,
            pending_output: Vec::new(),
            pending_start: 0,
            next_grain_source_frame: 0.0,
            first_grain: true,
            loop_enabled: false,
            loop_start_frame: None,
            loop_end_frame: None,
            channel_mode: AudioChannelMode::Stereo,
        }
    }
}

/// A thread-safe audio buffer for playback.
pub struct AudioBuffer {
    /// The decoded audio samples (interleaved) - immutable after creation.
    samples: Vec<f32>,
    /// Number of channels.
    channels: u16,
    /// Sample rate.
    sample_rate: u32,
    /// Total number of samples (cached for quick access).
    total_samples: usize,
    /// Grain size used by the pitch-preserving time stretcher.
    grain_size_frames: usize,
    /// Overlap used between grains.
    overlap_frames: usize,
    /// Output hop between grains.
    synthesis_hop_frames: usize,
    /// Candidate search radius for waveform alignment.
    search_radius_frames: usize,
    /// Mutable playback state shared between UI and audio callback.
    state: Mutex<PlaybackEngineState>,
}

impl AudioBuffer {
    /// Create a new audio buffer.
    pub fn new(samples: Vec<f32>, channels: u16, sample_rate: u32) -> Self {
        let total_samples = samples.len();
        let grain_size_frames =
            (((sample_rate as usize) * 40) / 1000).clamp(1_024, 4_096) / 64 * 64;
        let overlap_frames = (grain_size_frames / 4).max(128);
        let synthesis_hop_frames = grain_size_frames - overlap_frames;
        let search_radius_frames = (overlap_frames / 2).max(64);

        Self {
            samples,
            channels,
            sample_rate,
            total_samples,
            grain_size_frames,
            overlap_frames,
            synthesis_hop_frames,
            search_radius_frames,
            state: Mutex::new(PlaybackEngineState::default()),
        }
    }

    /// Get the sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get the total number of frames (samples per channel).
    pub fn frame_count(&self) -> usize {
        self.total_samples / self.channels as usize
    }

    /// Get the number of channels in the source audio.
    pub fn channel_count(&self) -> u16 {
        self.channels
    }

    /// Get current position in frames.
    pub fn position(&self) -> f64 {
        self.state.lock().display_position
    }

    /// Set position in frames.
    pub fn set_position(&self, frame: usize) {
        let mut state = self.state.lock();
        let frame = frame.min(self.frame_count());
        state.display_position = frame as f64;
        state.next_grain_source_frame = frame as f64;
        self.reset_stretcher_state(&mut state);
    }

    /// Set position from time.
    pub fn set_position_time(&self, time: Duration) {
        let frame = (time.as_secs_f64() * self.sample_rate as f64) as usize;
        self.set_position(frame);
    }

    /// Get current position as duration.
    pub fn position_time(&self) -> Duration {
        Duration::from_secs_f64(self.position() / self.sample_rate as f64)
    }

    /// Set playback speed.
    pub fn set_speed(&self, speed: f32) {
        let mut state = self.state.lock();
        let clamped = speed.clamp(0.1, 2.0);
        if (state.speed - clamped).abs() < 0.0001 {
            return;
        }

        state.speed = clamped;
        state.next_grain_source_frame = state.display_position;
        self.reset_stretcher_state(&mut state);
    }

    /// Get playback speed.
    pub fn speed(&self) -> f32 {
        self.state.lock().speed
    }

    /// Set source-channel playback mode.
    pub fn set_channel_mode(&self, mode: AudioChannelMode) {
        let mut state = self.state.lock();
        if state.channel_mode == mode {
            return;
        }

        state.channel_mode = mode;
        state.next_grain_source_frame = state.display_position;
        self.reset_stretcher_state(&mut state);
    }

    /// Get current source-channel playback mode.
    pub fn channel_mode(&self) -> AudioChannelMode {
        self.state.lock().channel_mode
    }

    /// Whether loop playback is currently active.
    pub fn loop_enabled(&self) -> bool {
        self.state.lock().loop_enabled
    }

    /// Get the current loop bounds in frames.
    pub fn loop_bounds(&self) -> Option<(usize, usize)> {
        let state = self.state.lock();
        if !state.loop_enabled {
            return None;
        }

        match (state.loop_start_frame, state.loop_end_frame) {
            (Some(start), Some(end)) if start < end => Some((start, end)),
            _ => None,
        }
    }

    /// Set loop region (thread-safe).
    pub fn set_loop(&self, start_frame: Option<usize>, end_frame: Option<usize>) {
        let mut state = self.state.lock();
        match (start_frame, end_frame) {
            (Some(start), Some(end)) if start < end => {
                let total_frames = self.frame_count();
                state.loop_start_frame = Some(start.min(total_frames));
                state.loop_end_frame = Some(end.min(total_frames));
                state.loop_enabled = true;
            }
            _ => {
                state.loop_enabled = false;
                state.loop_start_frame = None;
                state.loop_end_frame = None;
            }
        }

        state.next_grain_source_frame = state.display_position;
        self.reset_stretcher_state(&mut state);
    }

    /// Clear loop (thread-safe).
    pub fn clear_loop(&self) {
        let mut state = self.state.lock();
        state.loop_enabled = false;
        state.loop_start_frame = None;
        state.loop_end_frame = None;
        state.next_grain_source_frame = state.display_position;
        self.reset_stretcher_state(&mut state);
    }

    /// Check if at end.
    pub fn is_at_end(&self) -> bool {
        let state = self.state.lock();
        !state.loop_enabled && state.display_position >= self.frame_count() as f64
    }

    /// Reset to beginning.
    pub fn reset(&self) {
        let mut state = self.state.lock();
        state.display_position = 0.0;
        state.next_grain_source_frame = 0.0;
        self.reset_stretcher_state(&mut state);
    }
    /// Read samples with volume and pitch-preserving speed control.
    /// Returns the number of samples written to output.
    pub fn read_samples<T>(&self, count: usize, output: &mut [T], volume: f32) -> usize
    where
        T: Sample + cpal::SizedSample + cpal::FromSample<f32>,
    {
        if self.total_samples == 0 {
            return 0;
        }

        let max_samples = count.min(output.len());
        let channels = self.channels as usize;
        let frames_needed = max_samples / channels;

        if frames_needed == 0 {
            return 0;
        }

        let mut scratch = vec![0.0; frames_needed * channels];
        let mut state = self.state.lock();

        if (state.speed - 1.0).abs() < 0.0001 {
            self.render_direct(&mut state, &mut scratch);
        } else {
            self.ensure_stretched_output(&mut state, frames_needed);
            self.render_stretched(&mut state, &mut scratch);
        }

        for (dst, sample) in output.iter_mut().take(scratch.len()).zip(scratch.iter()) {
            *dst = T::from_sample(*sample * volume);
        }

        scratch.len()
    }

    fn render_direct(&self, state: &mut PlaybackEngineState, output: &mut [f32]) {
        let channels = self.channels as usize;
        let frames_needed = output.len() / channels;
        let mut source_pos = state.display_position;

        for frame in 0..frames_needed {
            let Some(resolved_frame) = self.resolve_source_frame(state, source_pos) else {
                for sample in output[frame * channels..].iter_mut() {
                    *sample = 0.0;
                }
                state.display_position = self.frame_count() as f64;
                return;
            };

            for ch in 0..channels {
                output[frame * channels + ch] = self.sample_at_frame(
                    state,
                    resolved_frame,
                    self.source_channel_for_output(state, ch),
                );
            }

            source_pos = self.advance_source_position(state, source_pos, 1.0);
        }

        state.display_position = source_pos;
    }

    fn ensure_stretched_output(&self, state: &mut PlaybackEngineState, frames_needed: usize) {
        let desired_frames = frames_needed + self.grain_size_frames;
        while self.pending_frames(state) < desired_frames {
            if !self.synthesize_next_grain(state) {
                break;
            }
        }
    }

    fn render_stretched(&self, state: &mut PlaybackEngineState, output: &mut [f32]) {
        let channels = self.channels as usize;
        let samples_needed = output.len();
        let available_samples = self.pending_samples(state).min(samples_needed);

        if available_samples > 0 {
            let start = state.pending_start;
            let end = start + available_samples;
            output[..available_samples].copy_from_slice(&state.pending_output[start..end]);
            state.pending_start = end;

            if state.pending_start >= state.pending_output.len() {
                state.pending_output.clear();
                state.pending_start = 0;
            }

            let frames_consumed = available_samples / channels;
            let delta = frames_consumed as f64 * state.speed as f64;
            state.display_position =
                self.advance_source_position(state, state.display_position, delta);
        }

        for sample in output[available_samples..].iter_mut() {
            *sample = 0.0;
        }

        self.compact_pending_output(state);
    }

    fn synthesize_next_grain(&self, state: &mut PlaybackEngineState) -> bool {
        if !state.loop_enabled && state.next_grain_source_frame >= self.frame_count() as f64 {
            return false;
        }

        let grain_start = if state.first_grain {
            state.next_grain_source_frame.max(0.0).round() as usize
        } else {
            self.find_best_grain_start(state)
        };

        let grain = self.extract_grain(state, grain_start);
        if grain.is_empty() {
            return false;
        }

        let channels = self.channels as usize;
        let overlap_samples = self.overlap_frames * channels;

        if state.first_grain || self.pending_frames(state) == 0 {
            state.pending_output.extend_from_slice(&grain);
            state.first_grain = false;
        } else {
            let overlap_start = state.pending_output.len().saturating_sub(overlap_samples);
            for frame in 0..self.overlap_frames {
                let fade = frame as f32 / self.overlap_frames as f32;
                for ch in 0..channels {
                    let sample_idx = frame * channels + ch;
                    let dst_idx = overlap_start + sample_idx;
                    let current = state.pending_output[dst_idx];
                    let incoming = grain[sample_idx];
                    state.pending_output[dst_idx] = current * (1.0 - fade) + incoming * fade;
                }
            }

            state
                .pending_output
                .extend_from_slice(&grain[overlap_samples..]);
        }

        let analysis_hop = self.synthesis_hop_frames as f64 * state.speed as f64;
        state.next_grain_source_frame =
            self.advance_source_position(state, grain_start as f64, analysis_hop);

        true
    }

    fn extract_grain(&self, state: &PlaybackEngineState, start_frame: usize) -> Vec<f32> {
        let channels = self.channels as usize;
        let mut grain = vec![0.0; self.grain_size_frames * channels];

        for frame in 0..self.grain_size_frames {
            let source_frame = start_frame as f64 + frame as f64;
            let Some(resolved_frame) = self.resolve_source_frame(state, source_frame) else {
                break;
            };

            for ch in 0..channels {
                grain[frame * channels + ch] = self.sample_at_frame(
                    state,
                    resolved_frame,
                    self.source_channel_for_output(state, ch),
                );
            }
        }

        grain
    }

    fn find_best_grain_start(&self, state: &PlaybackEngineState) -> usize {
        let channels = self.channels as usize;
        let overlap_samples = self.overlap_frames * channels;
        let tail_start = state.pending_output.len().saturating_sub(overlap_samples);
        let target = &state.pending_output[tail_start..];

        let expected = state.next_grain_source_frame.round() as isize;
        let mut best_start = expected.max(0) as usize;
        let mut best_score = f32::NEG_INFINITY;

        for offset in -(self.search_radius_frames as isize)..=(self.search_radius_frames as isize) {
            let candidate = (expected + offset).max(0) as usize;
            let score = self.overlap_similarity(state, target, candidate);
            if score > best_score {
                best_score = score;
                best_start = candidate;
            }
        }

        best_start
    }

    fn overlap_similarity(
        &self,
        state: &PlaybackEngineState,
        target: &[f32],
        candidate_start: usize,
    ) -> f32 {
        let channels = self.channels as usize;
        let mut dot = 0.0f32;
        let mut target_energy = 0.0f32;
        let mut candidate_energy = 0.0f32;

        for frame in 0..self.overlap_frames {
            let source_frame = candidate_start as f64 + frame as f64;
            let Some(resolved_frame) = self.resolve_source_frame(state, source_frame) else {
                break;
            };

            for ch in 0..channels {
                let idx = frame * channels + ch;
                let a = target[idx];
                let b = self.sample_at_frame(
                    state,
                    resolved_frame,
                    self.source_channel_for_output(state, ch),
                );
                dot += a * b;
                target_energy += a * a;
                candidate_energy += b * b;
            }
        }

        if target_energy <= 1e-9 || candidate_energy <= 1e-9 {
            dot
        } else {
            dot / (target_energy.sqrt() * candidate_energy.sqrt())
        }
    }

    fn sample_at_frame(&self, state: &PlaybackEngineState, frame: f64, channel: usize) -> f32 {
        let channels = self.channels as usize;
        let resolved = match self.resolve_source_frame(state, frame) {
            Some(resolved) => resolved,
            None => return 0.0,
        };

        let base = resolved.floor() as usize;
        let frac = (resolved - base as f64) as f32;
        let next = resolved.ceil() as usize;

        let sample_at = |frame_idx: usize| -> f32 {
            if frame_idx >= self.frame_count() {
                return 0.0;
            }

            let idx = frame_idx * channels + channel;
            self.samples.get(idx).copied().unwrap_or(0.0)
        };

        let s1 = sample_at(base);
        let s2 = sample_at(next);
        s1 * (1.0 - frac) + s2 * frac
    }

    fn resolve_source_frame(&self, state: &PlaybackEngineState, frame: f64) -> Option<f64> {
        if frame < 0.0 {
            return Some(0.0);
        }

        if let Some((loop_start, loop_end)) = self.valid_loop_bounds(state) {
            if frame < loop_end as f64 {
                return Some(frame);
            }

            let loop_len = (loop_end - loop_start) as f64;
            return Some(loop_start as f64 + (frame - loop_start as f64).rem_euclid(loop_len));
        }

        if frame >= self.frame_count() as f64 {
            None
        } else {
            Some(frame)
        }
    }

    fn source_channel_for_output(
        &self,
        state: &PlaybackEngineState,
        output_channel: usize,
    ) -> usize {
        let channels = self.channels as usize;
        if channels <= 1 {
            return 0;
        }

        match state.channel_mode {
            AudioChannelMode::Stereo => output_channel.min(channels - 1),
            AudioChannelMode::Left => 0,
            AudioChannelMode::Right => 1.min(channels - 1),
        }
    }

    fn advance_source_position(
        &self,
        state: &PlaybackEngineState,
        current: f64,
        delta: f64,
    ) -> f64 {
        if delta <= 0.0 {
            return current.max(0.0);
        }

        if let Some((loop_start, loop_end)) = self.valid_loop_bounds(state) {
            let loop_start = loop_start as f64;
            let loop_end = loop_end as f64;
            let loop_len = loop_end - loop_start;
            let mut position = current;
            let mut remaining = delta;

            if position < loop_start {
                let frames_before_wrap = (loop_end - position).max(0.0);
                if remaining < frames_before_wrap {
                    return position + remaining;
                }

                remaining -= frames_before_wrap;
                position = loop_start;
            }

            if position >= loop_end {
                position = loop_start + (position - loop_start).rem_euclid(loop_len);
            }

            return loop_start + (position + remaining - loop_start).rem_euclid(loop_len);
        }

        (current + delta).clamp(0.0, self.frame_count() as f64)
    }

    fn valid_loop_bounds(&self, state: &PlaybackEngineState) -> Option<(usize, usize)> {
        match (
            state.loop_enabled,
            state.loop_start_frame,
            state.loop_end_frame,
        ) {
            (true, Some(start), Some(end)) if start < end => Some((start, end)),
            _ => None,
        }
    }

    fn reset_stretcher_state(&self, state: &mut PlaybackEngineState) {
        state.pending_output.clear();
        state.pending_start = 0;
        state.first_grain = true;
    }

    fn pending_samples(&self, state: &PlaybackEngineState) -> usize {
        state
            .pending_output
            .len()
            .saturating_sub(state.pending_start)
    }

    fn pending_frames(&self, state: &PlaybackEngineState) -> usize {
        self.pending_samples(state) / self.channels as usize
    }

    fn compact_pending_output(&self, state: &mut PlaybackEngineState) {
        if state.pending_start == 0 {
            return;
        }

        if state.pending_start >= state.pending_output.len() {
            state.pending_output.clear();
            state.pending_start = 0;
            return;
        }

        if state.pending_start > 16_384 && state.pending_start * 2 >= state.pending_output.len() {
            state.pending_output.drain(..state.pending_start);
            state.pending_start = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate_zero_crossing_frequency(samples: &[f32], sample_rate: u32) -> f32 {
        let zero_crossings = samples
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();

        zero_crossings as f32 * sample_rate as f32 / samples.len() as f32
    }

    #[test]
    fn set_position_supports_longer_than_one_second_of_audio() {
        let buffer = AudioBuffer::new(vec![0.0; 90_000], 1, 48_000);

        buffer.set_position(70_000);

        assert_eq!(buffer.position() as usize, 70_000);
    }

    #[test]
    fn read_samples_advances_past_previous_u32_limit() {
        let samples: Vec<f32> = (0..100_000).map(|i| i as f32).collect();
        let buffer = AudioBuffer::new(samples, 1, 48_000);
        let mut output = vec![0.0f32; 70_000];

        let read = buffer.read_samples(output.len(), &mut output, 1.0);

        assert_eq!(read, output.len());
        assert!(buffer.position() >= 70_000.0);
        assert_eq!(output[69_999], 69_999.0);
    }

    #[test]
    fn loop_wrap_uses_loop_start_frame_instead_of_stale_index() {
        let buffer = AudioBuffer::new(vec![0.0, 1.0, 2.0, 3.0, 4.0], 1, 48_000);
        let mut output = vec![0.0f32; 2];

        buffer.set_loop(Some(1), Some(4));
        buffer.set_position(4);
        buffer.read_samples(output.len(), &mut output, 1.0);

        assert_eq!(output, vec![1.0, 2.0]);
    }

    #[test]
    fn slowed_playback_keeps_pitch_close_to_source() {
        let sample_rate = 48_000;
        let frequency = 440.0f32;
        let frames = sample_rate / 2;
        let mut source = Vec::with_capacity(frames as usize);

        for i in 0..frames {
            let phase = 2.0 * std::f32::consts::PI * frequency * i as f32 / sample_rate as f32;
            source.push(phase.sin());
        }

        let buffer = AudioBuffer::new(source, 1, sample_rate);
        buffer.set_speed(0.5);

        let mut output = vec![0.0f32; 12_000];
        buffer.read_samples(output.len(), &mut output, 1.0);

        let estimated = estimate_zero_crossing_frequency(&output[2_000..10_000], sample_rate);
        assert!(
            (estimated - frequency).abs() < 80.0,
            "expected pitch near {frequency}Hz, got {estimated}Hz"
        );
    }

    #[test]
    fn left_channel_mode_duplicates_left_source_across_stereo_output() {
        let buffer = AudioBuffer::new(vec![1.0, 10.0, 2.0, 20.0], 2, 48_000);
        buffer.set_channel_mode(AudioChannelMode::Left);

        let mut output = vec![0.0f32; 4];
        buffer.read_samples(output.len(), &mut output, 1.0);

        assert_eq!(output, vec![1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn right_channel_mode_duplicates_right_source_across_stereo_output() {
        let buffer = AudioBuffer::new(vec![1.0, 10.0, 2.0, 20.0], 2, 48_000);
        buffer.set_channel_mode(AudioChannelMode::Right);

        let mut output = vec![0.0f32; 4];
        buffer.read_samples(output.len(), &mut output, 1.0);

        assert_eq!(output, vec![10.0, 10.0, 20.0, 20.0]);
    }
}
