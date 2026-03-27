//! Audio playback using cpal
//!
//! Manages the audio output stream and coordinates playback

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use super::buffer::{AudioBuffer, AudioChannelMode};

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

/// Audio player that manages playback
pub struct AudioPlayer {
    /// The audio buffer to play from
    buffer: Arc<AudioBuffer>,
    /// Audio output stream
    stream: Option<Stream>,
    /// Current playback state
    state: AtomicU8,
    /// Volume (fixed-point: volume * 1000)
    volume: Arc<AtomicU32>,
    /// Playback speed (fixed-point: speed * 1000, 1000 = 1.0x)
    speed: Arc<AtomicU32>,
}

impl AudioPlayer {
    /// Create a new audio player
    pub fn new(buffer: Arc<AudioBuffer>) -> Result<Self> {
        Ok(Self {
            buffer,
            stream: None,
            state: AtomicU8::new(PlaybackState::Stopped as u8),
            volume: Arc::new(AtomicU32::new(1000)), // 1.0 volume
            speed: Arc::new(AtomicU32::new(1000)),  // 1.0 speed
        })
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
        let volume = self.volume.clone();
        tracing::debug!(
            "Audio device: {:?}, format: {:?}, channels: {}, sample_rate: {}",
            device.name(),
            sample_format,
            config.channels,
            config.sample_rate.0
        );

        // Create the output stream based on sample format
        match sample_format {
            SampleFormat::F32 => self.create_stream::<f32>(&device, &config, buffer, volume),
            SampleFormat::I16 => self.create_stream::<i16>(&device, &config, buffer, volume),
            SampleFormat::U16 => self.create_stream::<u16>(&device, &config, buffer, volume),
            _ => bail!("Unsupported sample format: {:?}", sample_format),
        }
    }

    fn create_stream<T>(
        &self,
        device: &Device,
        config: &StreamConfig,
        buffer: Arc<AudioBuffer>,
        volume: Arc<AtomicU32>,
    ) -> Result<Stream>
    where
        T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>,
    {
        let stream = device.build_output_stream(
            config,
            move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
                // Get current volume
                let vol = volume.load(Ordering::SeqCst) as f32 / 1000.0;

                // Read from buffer with volume
                let samples_read = buffer.read_samples(output.len(), output, vol);

                // Zero out any remaining samples if we reached end of buffer
                for i in samples_read..output.len() {
                    output[i] = T::from_sample(0.0);
                }
            },
            |err| eprintln!("Audio stream error: {}", err),
            None,
        )?;

        Ok(stream)
    }

    /// Start or resume playback
    pub fn play(&self) -> Result<()> {
        if self.buffer.is_at_end() {
            self.buffer.reset();
        }

        self.state
            .store(PlaybackState::Playing as u8, Ordering::SeqCst);
        if let Some(stream) = &self.stream {
            stream.play()?;
        }
        Ok(())
    }

    /// Pause playback
    pub fn pause(&self) -> Result<()> {
        self.state
            .store(PlaybackState::Paused as u8, Ordering::SeqCst);
        if let Some(stream) = &self.stream {
            stream.pause()?;
        }
        Ok(())
    }

    /// Stop playback and reset position
    pub fn stop(&self) {
        self.state
            .store(PlaybackState::Stopped as u8, Ordering::SeqCst);
        self.buffer.reset();
        if let Some(stream) = &self.stream {
            let _ = stream.pause();
        }
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
    pub fn speed(&self) -> f32 {
        self.speed.load(Ordering::SeqCst) as f32 / 1000.0
    }

    /// Set source-channel playback mode.
    pub fn set_channel_mode(&self, mode: AudioChannelMode) {
        self.buffer.set_channel_mode(mode);
    }

    /// Get current source-channel playback mode.
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
}

/// Use anyhow's bail! macro
use anyhow::bail;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_preserves_selected_speed() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 32], 2, 48_000));
        let player =
            AudioPlayer::new(buffer.clone()).expect("player should initialize without a stream");

        player.set_speed(1.5);
        player.stop();

        assert!((player.speed() - 1.5).abs() < 0.001);
        assert!((buffer.speed() - 1.5).abs() < 0.001);
    }

    #[test]
    fn play_from_end_restarts_from_beginning() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 16], 2, 48_000));
        let player =
            AudioPlayer::new(buffer.clone()).expect("player should initialize without a stream");

        buffer.set_position(buffer.frame_count());
        assert!(buffer.is_at_end());

        player.play().expect("play should succeed without a stream");

        assert_eq!(buffer.position_time(), std::time::Duration::ZERO);
    }

    #[test]
    fn channel_mode_updates_are_forwarded_to_buffer() {
        let buffer = Arc::new(AudioBuffer::new(vec![0.0; 16], 2, 48_000));
        let player =
            AudioPlayer::new(buffer.clone()).expect("player should initialize without a stream");

        player.set_channel_mode(AudioChannelMode::Right);

        assert_eq!(player.channel_mode(), AudioChannelMode::Right);
        assert_eq!(buffer.channel_mode(), AudioChannelMode::Right);
    }
}
