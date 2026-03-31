//! Waveform data generation for visualization

use parking_lot::RwLock;
use std::sync::Arc;

/// Represents a single point in the waveform display
#[derive(Debug, Clone, Copy)]
pub struct WaveformPoint {
    /// Minimum sample value in this region
    pub min: f32,
    /// Maximum sample value in this region
    pub max: f32,
}

/// Waveform data at a specific resolution
#[derive(Debug, Clone)]
pub struct WaveformLevel {
    /// Points representing the waveform
    pub points: Vec<WaveformPoint>,
}

#[derive(Debug, Clone)]
struct CachedWaveformWindow {
    start_frame: usize,
    end_frame: usize,
    requested_width: usize,
    level: WaveformLevel,
}

/// Multi-resolution waveform cache
pub struct WaveformGenerator {
    /// Decoded audio samples
    samples: Arc<[f32]>,
    /// Number of channels
    channels: u16,
    /// Total number of frames in the source audio.
    total_frames: usize,
    /// Cached waveform windows for recent viewports.
    windows: RwLock<Vec<CachedWaveformWindow>>,
}

impl WaveformGenerator {
    /// Create a new waveform generator
    pub fn new<S>(samples: S, channels: u16, _sample_rate: u32) -> Self
    where
        S: Into<Arc<[f32]>>,
    {
        let samples = samples.into();
        Self {
            total_frames: samples.len() / channels as usize,
            samples,
            channels,
            windows: RwLock::new(Vec::new()),
        }
    }

    /// Get total number of frames in the source audio.
    pub fn frame_count(&self) -> usize {
        self.total_frames
    }

    /// Generate waveform data for a specific frame window.
    pub fn generate_window(
        &self,
        start_frame: usize,
        end_frame: usize,
        pixels_width: usize,
    ) -> WaveformLevel {
        let samples = &self.samples;
        let total_frames = self.total_frames;

        if total_frames == 0 || pixels_width == 0 {
            return WaveformLevel { points: Vec::new() };
        }

        let start_frame = start_frame.min(total_frames);
        let end_frame = end_frame.clamp(start_frame.saturating_add(1), total_frames);
        let window_frames = end_frame - start_frame;
        let num_points = pixels_width.max(1);

        let mut points = Vec::with_capacity(num_points);

        let channels = self.channels as usize;

        for i in 0..num_points {
            let start = start_frame + i * window_frames / num_points;
            let mut end = start_frame + (i + 1) * window_frames / num_points;
            if end <= start {
                end = (start + 1).min(end_frame);
            }

            let mut min = f32::MAX;
            let mut max = f32::MIN;

            for frame in start..end {
                // Mix all channels to mono for visualization
                let mut mono = 0.0;
                for ch in 0..channels {
                    mono += samples[frame * channels + ch];
                }
                mono /= channels as f32;

                min = min.min(mono);
                max = max.max(mono);
            }

            points.push(WaveformPoint {
                min: if min == f32::MAX { 0.0 } else { min },
                max: if max == f32::MIN { 0.0 } else { max },
            });
        }

        WaveformLevel { points }
    }

    /// Get cached waveform data for the current visible frame window.
    pub fn get_window(
        &self,
        start_frame: usize,
        end_frame: usize,
        desired_width: usize,
    ) -> Option<WaveformLevel> {
        if desired_width == 0 || self.total_frames == 0 {
            return None;
        }

        let start_frame = start_frame.min(self.total_frames);
        let end_frame = end_frame.clamp(start_frame.saturating_add(1), self.total_frames);

        {
            let windows = self.windows.read();

            if let Some(window) = windows.iter().find(|window| {
                window.start_frame == start_frame
                    && window.end_frame == end_frame
                    && window.requested_width == desired_width
            }) {
                return Some(window.level.clone());
            }
        }

        let generated = self.generate_window(start_frame, end_frame, desired_width);
        let mut windows = self.windows.write();
        windows.push(CachedWaveformWindow {
            start_frame,
            end_frame,
            requested_width: desired_width,
            level: generated.clone(),
        });

        const MAX_CACHED_WINDOWS: usize = 8;
        if windows.len() > MAX_CACHED_WINDOWS {
            let excess = windows.len() - MAX_CACHED_WINDOWS;
            windows.drain(..excess);
        }

        Some(generated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_window_generation_preserves_local_peaks() {
        let samples = vec![
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, -1.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let generator = WaveformGenerator::new(samples, 1, 48_000);

        let level = generator
            .get_window(10, 12, 8)
            .expect("windowed waveform should be generated");

        assert_eq!(level.points.len(), 8);
        assert!(level.points.iter().any(|point| point.max == 1.0));
        assert!(level.points.iter().any(|point| point.min == -1.0));
    }

    #[test]
    fn window_generation_matches_requested_pixel_width() {
        let generator = WaveformGenerator::new(vec![0.0; 128], 1, 48_000);

        let level = generator
            .get_window(0, 128, 37)
            .expect("windowed waveform should be generated");

        assert_eq!(level.points.len(), 37);
    }
}
