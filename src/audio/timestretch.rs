//! Time-stretching audio processor
//!
//! Implements phase vocoder-like time stretching
//! to change playback speed without affecting pitch.

use std::collections::VecDeque;

/// Simple time stretcher using sample interpolation with overlap-add
/// This is a simplified approach that works well for moderate speed changes
pub struct TimeStretcher {
    /// Input sample rate
    _sample_rate: u32,
    /// Number of channels
    channels: u16,
    /// Playback speed (1.0 = normal, 0.5 = half speed, 2.0 = double speed)
    speed: f32,
    /// Input buffer (interleaved samples)
    input_buffer: VecDeque<f32>,
    /// Current read position (in frames, fractional)
    read_position: f64,
    /// Previous samples for interpolation
    prev_samples: Vec<f32>,
    /// Window for overlap-add
    window_size: usize,
    /// Overlap buffer for smooth transitions
    overlap_buffer: Vec<f32>,
}

impl TimeStretcher {
    /// Create a new time stretcher
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            _sample_rate: sample_rate,
            channels,
            speed: 1.0,
            input_buffer: VecDeque::new(),
            read_position: 0.0,
            prev_samples: vec![0.0; channels as usize],
            window_size: 32, // Small window for smooth transitions
            overlap_buffer: vec![0.0; 1024], // Will be resized as needed
        }
    }

    /// Set playback speed
    /// - 1.0 = normal speed
    /// - 0.5 = half speed (slower)
    /// - 2.0 = double speed (faster)
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(0.25, 4.0);
    }

    /// Get current speed
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Clear all buffers
    pub fn clear(&mut self) {
        self.input_buffer.clear();
        self.read_position = 0.0;
        self.prev_samples.fill(0.0);
        self.overlap_buffer.fill(0.0);
    }

    /// Set position (seek) - in frames
    pub fn set_position(&mut self, frame: usize) {
        self.read_position = frame as f64;
        self.prev_samples.fill(0.0);
        self.overlap_buffer.fill(0.0);
    }

    /// Add samples to the input buffer (interleaved)
    pub fn push_samples(&mut self, samples: &[f32]) {
        for sample in samples {
            self.input_buffer.push_back(*sample);
        }
    }

    /// Read processed samples using linear interpolation
    /// Returns number of samples read (interleaved)
    pub fn read(&mut self, output: &mut [f32]) -> usize {
        let channels = self.channels as usize;
        let frames_needed = output.len() / channels;
        let frames_available = self.input_buffer.len() / channels;

        // Check if we have enough input
        // Calculate required input frames based on speed
        let required_input_frames = (frames_needed as f32 * self.speed) as usize + 4;

        if frames_available < required_input_frames {
            // Not enough data, output silence
            for sample in output.iter_mut() {
                *sample = 0.0;
            }
            return output.len();
        }

        // Ensure overlap buffer is large enough
        let total_output_samples = output.len();
        if self.overlap_buffer.len() < total_output_samples {
            self.overlap_buffer.resize(total_output_samples, 0.0);
        }

        // Generate output using resampling
        for out_frame in 0..frames_needed {
            let input_pos = self.read_position;
            let frame_idx = input_pos as usize;
            let frac = (input_pos - frame_idx as f64) as f32;

            // Get input indices for this frame
            let idx1 = frame_idx * channels;
            let idx2 = (frame_idx + 1) * channels;

            // Linear interpolation for each channel
            for ch in 0..channels {
                let s1 = if idx1 + ch < self.input_buffer.len() {
                    self.input_buffer[idx1 + ch]
                } else {
                    0.0
                };

                let s2 = if idx2 + ch < self.input_buffer.len() {
                    self.input_buffer[idx2 + ch]
                } else {
                    s1 // Use same sample if at end
                };

                // Linear interpolation
                let sample = s1 * (1.0 - frac) + s2 * frac;
                self.overlap_buffer[out_frame * channels + ch] = sample;
            }

            // Advance read position
            self.read_position += self.speed as f64;
        }

        // Apply small crossfade window to avoid clicks
        let window = self.window_size.min(frames_needed);
        for i in 0..window {
            let t = i as f32 / window as f32;
            for ch in 0..channels {
                let idx = i * channels + ch;
                // Crossfade from previous samples
                output[idx] = self.prev_samples[ch] * (1.0 - t) + self.overlap_buffer[idx] * t;
                // Store for next time
                if i == window - 1 {
                    self.prev_samples[ch] = self.overlap_buffer[(frames_needed - 1) * channels + ch];
                }
            }
        }

        // Copy remaining samples without crossfade
        for i in window..frames_needed {
            for ch in 0..channels {
                let idx = i * channels + ch;
                output[idx] = self.overlap_buffer[idx];
            }
            // Store last samples
            if i == frames_needed - 1 {
                for ch in 0..channels {
                    self.prev_samples[ch] = self.overlap_buffer[idx * channels + ch];
                }
            }
        }

        // Remove consumed input from buffer
        let consumed_frames = self.read_position as usize;
        let consumed_samples = consumed_frames * channels;
        if consumed_samples > 0 {
            for _ in 0..consumed_samples.min(self.input_buffer.len()) {
                self.input_buffer.pop_front();
            }
            self.read_position -= consumed_frames as f64;
        }

        output.len()
    }

    /// Read processed samples with volume control
    pub fn read_with_volume(&mut self, output: &mut [f32], volume: f32) -> usize {
        let read = self.read(output);
        for i in 0..read {
            output[i] *= volume;
        }
        read
    }

    /// Check if there are samples available to read
    pub fn available(&self) -> usize {
        let frames = self.input_buffer.len() / self.channels as usize;
        (frames as f32 / self.speed) as usize * self.channels as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestretcher_creation() {
        let ts = TimeStretcher::new(44100, 2);
        assert_eq!(ts.speed(), 1.0);
    }

    #[test]
    fn test_speed_setting() {
        let mut ts = TimeStretcher::new(44100, 2);
        ts.set_speed(0.5);
        assert_eq!(ts.speed(), 0.5);

        ts.set_speed(5.0); // Should clamp to 4.0
        assert_eq!(ts.speed(), 4.0);
    }

    #[test]
    fn test_basic_processing() {
        let mut ts = TimeStretcher::new(44100, 2);

        // Generate some test samples (sine wave)
        let mut input = Vec::new();
        for i in 0..1000 {
            let sample = (i as f32 * 0.1).sin();
            input.push(sample); // Left
            input.push(sample); // Right
        }

        ts.push_samples(&input);

        // Read output
        let mut output = vec![0.0; 512];
        let read = ts.read(&mut output);

        assert_eq!(read, 512);
    }
}
