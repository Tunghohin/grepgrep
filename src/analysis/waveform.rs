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

/// Multi-resolution waveform cache
pub struct WaveformGenerator {
    /// Decoded audio samples
    samples: Arc<RwLock<Vec<f32>>>,
    /// Number of channels
    channels: u16,
    /// Cached waveform levels (multi-resolution)
    levels: RwLock<Vec<WaveformLevel>>,
}

impl WaveformGenerator {
    /// Create a new waveform generator
    pub fn new(samples: Vec<f32>, channels: u16, _sample_rate: u32) -> Self {
        Self {
            samples: Arc::new(RwLock::new(samples)),
            channels,
            levels: RwLock::new(Vec::new()),
        }
    }

    /// Generate waveform data for a specific resolution
    pub fn generate(&self, pixels_width: usize) -> WaveformLevel {
        let samples = self.samples.read();
        let total_frames = samples.len() / self.channels as usize;

        if total_frames == 0 || pixels_width == 0 {
            return WaveformLevel { points: Vec::new() };
        }

        let samples_per_pixel = (total_frames as f64 / pixels_width as f64).ceil() as usize;
        let num_points = (total_frames + samples_per_pixel - 1) / samples_per_pixel;

        let mut points = Vec::with_capacity(num_points);

        let channels = self.channels as usize;

        for i in 0..num_points {
            let start = i * samples_per_pixel;
            let end = ((i + 1) * samples_per_pixel).min(total_frames);

            let mut min = f32::MAX;
            let mut max = f32::MIN;
            let mut sum_sq = 0.0;
            let mut count = 0;

            for frame in start..end {
                // Mix all channels to mono for visualization
                let mut mono = 0.0;
                for ch in 0..channels {
                    mono += samples[frame * channels + ch];
                }
                mono /= channels as f32;

                min = min.min(mono);
                max = max.max(mono);
                sum_sq += mono * mono;
                count += 1;
            }

            let _rms = if count > 0 {
                (sum_sq / count as f32).sqrt()
            } else {
                0.0
            };

            points.push(WaveformPoint {
                min: if min == f32::MAX { 0.0 } else { min },
                max: if max == f32::MIN { 0.0 } else { max },
            });
        }

        WaveformLevel { points }
    }

    /// Generate and cache multiple resolution levels
    pub fn generate_multi_resolution(&self, max_width: usize) {
        let mut levels = self.levels.write();
        levels.clear();

        // Generate at different zoom levels
        let widths = [200, 400, 800, 1600, 3200, 6400, 12800];

        for &width in &widths {
            if width <= max_width {
                levels.push(self.generate(width));
            }
        }
    }

    /// Get cached level closest to desired width
    pub fn get_level(&self, desired_width: usize) -> Option<WaveformLevel> {
        let levels = self.levels.read();

        if levels.is_empty() {
            return None;
        }

        // Find the level with enough points to cover the desired width
        // Prefer the smallest level that has at least desired_width points
        for level in levels.iter() {
            if level.points.len() >= desired_width {
                return Some(level.clone());
            }
        }

        // If no level has enough points, return the highest resolution one
        levels.last().cloned()
    }
}
