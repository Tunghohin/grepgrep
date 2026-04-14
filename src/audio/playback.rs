//! Audio playback using cpal
//!
//! Manages the audio output stream and coordinates playback

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use super::buffer::{AudioBuffer, AudioChannelMode};

const REFERENCE_TONE_AMPLITUDE: f32 = 0.18;
const REFERENCE_TONE_DURATION_SECS: f32 = 1.0;
const REFERENCE_TONE_ATTACK_SECS: f32 = 0.010;
const REFERENCE_TONE_DECAY_CURVE: f32 = 6.0;

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped = 0,
    Playing = 1,
    Paused = 2,
}

impl From<u8> for PlaybackState {
    fn from(v: u8) -> Self {
        match v {
            0 => PlaybackState::Stopped,
            1 => PlaybackState::Playing,
            2 => PlaybackState::Paused,
            _ => PlaybackState::Stopped,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ReferenceToneState {
    sample_rate: u32,
    active_midi_note: Option<i32>,
    phase: f32,
    phase_increment: f32,
    sample_index: u64,
    total_samples: u64,
    attack_samples: u64,
}

impl ReferenceToneState {
    fn set_output_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
    }

    fn trigger(&mut self, midi_note: i32) {
        let sample_rate = self.sample_rate.max(48_000);
        self.active_midi_note = Some(midi_note);
        self.phase = 0.0;
        self.phase_increment =
            std::f32::consts::TAU * midi_to_frequency(midi_note as f32) / sample_rate as f32;
        self.sample_index = 0;
        self.total_samples = (sample_rate as f32 * REFERENCE_TONE_DURATION_SECS).round() as u64;
        self.attack_samples = ((sample_rate as f32 * REFERENCE_TONE_ATTACK_SECS).round() as u64)
            .max(1)
            .min(self.total_samples.max(1));
    }

    fn active_midi_note(&self) -> Option<i32> {
        (self.sample_index < self.total_samples)
            .then_some(self.active_midi_note)
            .flatten()
    }

    fn clear(&mut self) {
        self.active_midi_note = None;
        self.phase = 0.0;
        self.phase_increment = 0.0;
        self.sample_index = 0;
        self.total_samples = 0;
        self.attack_samples = 0;
    }

    fn mix_into(&mut self, output: &mut [f32], output_channels: usize, volume: f32) {
        let output_channels = output_channels.max(1);
        let frame_count = output.len() / output_channels;
        if frame_count == 0 {
            return;
        }

        let Some(_) = self.active_midi_note() else {
            return;
        };

        let decay_samples = self
            .total_samples
            .saturating_sub(self.attack_samples)
            .max(1);
        for frame in 0..frame_count {
            if self.sample_index >= self.total_samples {
                self.active_midi_note = None;
                break;
            }

            let envelope = if self.sample_index < self.attack_samples {
                self.sample_index as f32 / self.attack_samples as f32
            } else {
                let decay_progress =
                    (self.sample_index - self.attack_samples) as f32 / decay_samples as f32;
                (-REFERENCE_TONE_DECAY_CURVE * decay_progress).exp()
            };
            let sample = self.phase.sin() * envelope * REFERENCE_TONE_AMPLITUDE * volume;
            let frame_start = frame * output_channels;
            for channel in 0..output_channels {
                let index = frame_start + channel;
                output[index] = (output[index] + sample).clamp(-1.0, 1.0);
            }

            self.phase = (self.phase + self.phase_increment).rem_euclid(std::f32::consts::TAU);
            self.sample_index += 1;
        }

        if self.sample_index >= self.total_samples {
            self.active_midi_note = None;
        }
    }
}

/// Audio player that manages playback
pub struct AudioPlayer {
    /// The audio buffer to play from
    buffer: Arc<AudioBuffer>,
    /// Audio output stream
    stream: Option<Stream>,
    /// Current playback state
    state: Arc<AtomicU8>,
    /// Volume (fixed-point: volume * 1000)
    volume: Arc<AtomicU32>,
    /// Playback speed (fixed-point: speed * 1000, 1000 = 1.0x)
    speed: Arc<AtomicU32>,
    /// One-shot reference tone mixed into the main output callback.
    reference_tone: Arc<Mutex<ReferenceToneState>>,
}

impl AudioPlayer {
    /// Create a new audio player
    pub fn new(buffer: Arc<AudioBuffer>) -> Self {
        Self {
            buffer,
            stream: None,
            state: Arc::new(AtomicU8::new(PlaybackState::Stopped as u8)),
            volume: Arc::new(AtomicU32::new(1000)), // 1.0 volume
            speed: Arc::new(AtomicU32::new(1000)),  // 1.0 speed
            reference_tone: Arc::new(Mutex::new(ReferenceToneState::default())),
        }
    }

    /// Initialize the audio stream
    pub fn init_stream(&mut self) -> Result<()> {
        // Try to find a working audio host
        let hosts = cpal::available_hosts();
        let mut last_error = None;

        for host_id in hosts {
            if let Ok(host) = cpal::host_from_id(host_id) {
                match self.try_init_with_host(&host) {
                    Ok(stream) => {
                        tracing::info!("Successfully initialized audio with host: {:?}", host_id);
                        self.stream = Some(stream);
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::debug!("Failed to initialize with host {:?}: {}", host_id, e);
                        last_error = Some(e);
                    }
                }
            }
        }

        // If we get here, no host worked
        let error_msg = last_error
            .map(|e| format!("No working audio device found. Last error: {}", e))
            .unwrap_or_else(|| "No audio hosts available".to_string());

        // Provide helpful message for WSL2 users
        let help_msg = if cfg!(target_os = "linux") {
            "\n\nOn WSL2, you may need to configure PulseAudio. Try:\
             1. Install PulseAudio on Windows\
             2. Add 'export PULSE_SERVER=tcp:$(hostname).local' to ~/.bashrc\
             3. Or use: pipewire-pulse for PipeWire support"
        } else {
            ""
        };

        Err(anyhow::anyhow!("{}{}", error_msg, help_msg))
    }

    fn try_init_with_host(&self, host: &cpal::Host) -> Result<Stream> {
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No output device found"))?;

        let supported_config = device.default_output_config()?;
        let sample_format = supported_config.sample_format();
        let config: StreamConfig = supported_config.into();

        let buffer = self.buffer.clone();
        buffer.set_output_channel_count(config.channels as usize);
        let volume = self.volume.clone();
        self.reference_tone
            .lock()
            .set_output_sample_rate(config.sample_rate.0);
        tracing::debug!(
            "Audio device: {:?}, format: {:?}, channels: {}, sample_rate: {}",
            device.name(),
            sample_format,
            config.channels,
            config.sample_rate.0
        );

        // Create the output stream based on sample format
        match sample_format {
            SampleFormat::F32 => self.create_stream::<f32>(
                &device,
                &config,
                buffer,
                volume,
                self.state.clone(),
                self.reference_tone.clone(),
            ),
            SampleFormat::I16 => self.create_stream::<i16>(
                &device,
                &config,
                buffer,
                volume,
                self.state.clone(),
                self.reference_tone.clone(),
            ),
            SampleFormat::U16 => self.create_stream::<u16>(
                &device,
                &config,
                buffer,
                volume,
                self.state.clone(),
                self.reference_tone.clone(),
            ),
            _ => bail!("Unsupported sample format: {:?}", sample_format),
        }
    }

    fn create_stream<T>(
        &self,
        device: &Device,
        config: &StreamConfig,
        buffer: Arc<AudioBuffer>,
        volume: Arc<AtomicU32>,
        playback_state: Arc<AtomicU8>,
        reference_tone: Arc<Mutex<ReferenceToneState>>,
    ) -> Result<Stream>
    where
        T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
    {
        let output_channels = config.channels as usize;
        let mut scratch = Vec::<f32>::new();
        let stream = device.build_output_stream(
            config,
            move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
                let vol = volume.load(Ordering::SeqCst) as f32 / 1000.0;
                if scratch.len() < output.len() {
                    scratch.resize(output.len(), 0.0);
                }
                mix_output_block(
                    &buffer,
                    &mut scratch[..output.len()],
                    output_channels,
                    vol,
                    PlaybackState::from(playback_state.load(Ordering::SeqCst)),
                    reference_tone.as_ref(),
                );

                for (sample, mixed) in output.iter_mut().zip(scratch.iter()) {
                    *sample = T::from_sample(*mixed);
                }
            },
            |err| tracing::error!("Audio stream error: {}", err),
            None,
        )?;

        Ok(stream)
    }

    /// Start or resume playback
    pub fn play(&self) -> Result<()> {
        if self.buffer.is_at_end() {
            self.buffer.reset();
        }

        self.ensure_stream_running()?;
        self.state
            .store(PlaybackState::Playing as u8, Ordering::SeqCst);
        Ok(())
    }

    /// Pause playback
    pub fn pause(&self) -> Result<()> {
        self.state
            .store(PlaybackState::Paused as u8, Ordering::SeqCst);
        Ok(())
    }

    /// Stop playback and reset position
    pub fn stop(&self) {
        self.state
            .store(PlaybackState::Stopped as u8, Ordering::SeqCst);
        self.buffer.reset();
        self.reference_tone.lock().clear();
    }

    /// Get current playback state
    pub fn state(&self) -> PlaybackState {
        self.state.load(Ordering::SeqCst).into()
    }

    /// Check if playing
    pub fn is_playing(&self) -> bool {
        self.state() == PlaybackState::Playing
    }

    /// Set volume
    pub fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.volume
            .store((clamped * 1000.0) as u32, Ordering::SeqCst);
    }

    /// Set playback speed
    /// - 1.0 = normal speed
    /// - 0.5 = half speed (slower)
    /// - 2.0 = double speed (faster)
    pub fn set_speed(&self, speed: f32) {
        let clamped = speed.clamp(0.1, 2.0);
        self.speed
            .store((clamped * 1000.0) as u32, Ordering::SeqCst);
        self.buffer.set_speed(clamped);
    }

    /// Get playback speed
    #[cfg(test)]
    pub fn speed(&self) -> f32 {
        self.speed.load(Ordering::SeqCst) as f32 / 1000.0
    }

    /// Set source-channel playback mode.
    pub fn set_channel_mode(&self, mode: AudioChannelMode) {
        self.buffer.set_channel_mode(mode);
    }

    /// Trigger a one-shot sine reference tone for the given MIDI note.
    pub fn trigger_reference_tone(&self, midi_note: i32) {
        if let Err(error) = self.ensure_stream_running() {
            tracing::error!("Failed to start audio output for reference tone: {}", error);
        }
        self.reference_tone.lock().trigger(midi_note);
    }

    /// Get the currently active reference tone, if any.
    pub fn active_reference_tone_midi(&self) -> Option<i32> {
        self.reference_tone.lock().active_midi_note()
    }

    /// Get current source-channel playback mode.
    #[cfg(test)]
    pub fn channel_mode(&self) -> AudioChannelMode {
        self.buffer.channel_mode()
    }

    /// Seek to position (in duration)
    pub fn seek_time(&self, time: std::time::Duration) {
        self.buffer.set_position_time(time);
    }

    /// Get current position as duration
    pub fn position_time(&self) -> std::time::Duration {
        self.buffer.position_time()
    }

    #[cfg(test)]
    fn mix_reference_tone_for_test(&self, output: &mut [f32], output_channels: usize, volume: f32) {
        self.reference_tone
            .lock()
            .mix_into(output, output_channels, volume);
    }

    #[cfg(test)]
    fn mix_output_for_test(&self, output: &mut [f32], output_channels: usize, volume: f32) {
        mix_output_block(
            &self.buffer,
            output,
            output_channels,
            volume,
            self.state(),
            self.reference_tone.as_ref(),
        );
    }

    fn ensure_stream_running(&self) -> Result<()> {
        if let Some(stream) = &self.stream {
            stream.play()?;
        }
        Ok(())
    }
}

/// Use anyhow's bail! macro
use anyhow::bail;

fn midi_to_frequency(midi: f32) -> f32 {
    440.0 * 2.0_f32.powf((midi - 69.0) / 12.0)
}

fn mix_output_block(
    buffer: &AudioBuffer,
    output: &mut [f32],
    output_channels: usize,
    volume: f32,
    playback_state: PlaybackState,
    reference_tone: &Mutex<ReferenceToneState>,
) {
    output.fill(0.0);

    if playback_state == PlaybackState::Playing {
        let _ = buffer.read_samples(output.len(), output, volume);
    }

    reference_tone
        .lock()
        .mix_into(output, output_channels, volume);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_preserves_selected_speed() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 32], 2, 48_000));
        let player = AudioPlayer::new(buffer.clone());

        player.set_speed(1.5);
        player.stop();

        assert!((player.speed() - 1.5).abs() < 0.001);
        assert!((buffer.speed() - 1.5).abs() < 0.001);
    }

    #[test]
    fn play_from_end_restarts_from_beginning() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 16], 2, 48_000));
        let player = AudioPlayer::new(buffer.clone());

        buffer.set_position(buffer.frame_count());
        assert!(buffer.is_at_end());

        player.play().expect("play should succeed without a stream");

        assert_eq!(buffer.position_time(), std::time::Duration::ZERO);
    }

    #[test]
    fn channel_mode_updates_are_forwarded_to_buffer() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 16], 2, 48_000));
        let player = AudioPlayer::new(buffer.clone());

        player.set_channel_mode(AudioChannelMode::Right);

        assert_eq!(player.channel_mode(), AudioChannelMode::Right);
        assert_eq!(buffer.channel_mode(), AudioChannelMode::Right);
    }

    #[test]
    fn reference_tone_generates_audio_then_expires() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 16], 2, 48_000));
        let player = AudioPlayer::new(buffer);
        player.reference_tone.lock().set_output_sample_rate(48_000);

        player.trigger_reference_tone(69);
        assert_eq!(player.active_reference_tone_midi(), Some(69));

        let mut output = vec![0.0_f32; 48_000 * 2];
        player.mix_reference_tone_for_test(&mut output, 2, 1.0);

        assert!(output.iter().any(|sample| sample.abs() > 0.0001));
        assert_eq!(player.active_reference_tone_midi(), None);
    }

    #[test]
    fn retriggering_reference_tone_replaces_note_and_resets_envelope() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 16], 2, 48_000));
        let player = AudioPlayer::new(buffer);
        player.reference_tone.lock().set_output_sample_rate(48_000);

        player.trigger_reference_tone(69);
        let mut output = vec![0.0_f32; 512];
        player.mix_reference_tone_for_test(&mut output, 1, 1.0);

        let phase_increment_before = player.reference_tone.lock().phase_increment;
        assert!(player.reference_tone.lock().sample_index > 0);

        player.trigger_reference_tone(72);
        let state = player.reference_tone.lock().clone();
        let phase_increment_after = state.phase_increment;

        assert_eq!(state.active_midi_note, Some(72));
        assert_eq!(state.sample_index, 0);
        assert!(phase_increment_after > phase_increment_before);
    }

    #[test]
    fn reference_tone_mix_is_clamped_when_audio_is_already_loud() {
        let buffer = Arc::new(AudioBuffer::new(vec![1.0; 4_096], 2, 48_000));
        let player = AudioPlayer::new(buffer.clone());
        player.reference_tone.lock().set_output_sample_rate(48_000);
        player.trigger_reference_tone(69);

        let mut output = vec![0.0_f32; 2_048];
        let _ = buffer.read_samples(output.len(), &mut output, 1.0);
        player.mix_reference_tone_for_test(&mut output, 2, 1.0);

        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(output
            .iter()
            .all(|sample| *sample <= 1.0 && *sample >= -1.0));
    }

    #[test]
    fn stopped_transport_keeps_program_audio_silent() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.6; 64], 2, 48_000));
        let player = AudioPlayer::new(buffer.clone());
        let mut output = vec![0.0_f32; 64];

        let start_position = buffer.position_time();
        player.mix_output_for_test(&mut output, 2, 1.0);

        assert!(output.iter().all(|sample| sample.abs() <= 0.0001));
        assert_eq!(buffer.position_time(), start_position);
    }

    #[test]
    fn reference_tone_plays_while_transport_is_stopped() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 64], 2, 48_000));
        let player = AudioPlayer::new(buffer.clone());
        player.reference_tone.lock().set_output_sample_rate(48_000);
        player.trigger_reference_tone(69);

        let start_position = buffer.position_time();
        let mut output = vec![0.0_f32; 2_048];
        player.mix_output_for_test(&mut output, 2, 1.0);

        assert!(output.iter().any(|sample| sample.abs() > 0.0001));
        assert_eq!(buffer.position_time(), start_position);
    }
}
