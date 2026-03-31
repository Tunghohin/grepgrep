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
struct CachedWaveformLevel {
    requested_width: usize,
    level: WaveformLevel,
}

/// Multi-resolution waveform cache
pub struct WaveformGenerator {
    /// Decoded audio samples
    samples: Arc<[f32]>,
    /// Number of channels
    channels: u16,
    /// Cached waveform levels (multi-resolution)
    levels: RwLock<Vec<CachedWaveformLevel>>,
}

impl WaveformGenerator {
    /// Create a new waveform generator
    pub fn new<S>(samples: S, channels: u16, _sample_rate: u32) -> Self
    where
        S: Into<Arc<[f32]>>,
    {
        Self {
            samples: samples.into(),
            channels,
            levels: RwLock::new(Vec::new()),
        }
    }

    /// Generate waveform data for a specific resolution
    pub fn generate(&self, pixels_width: usize) -> WaveformLevel {
        let samples = &self.samples;
        let total_frames = samples.len() / self.channels as usize;

        if total_frames == 0 || pixels_width == 0 {
            return WaveformLevel { points: Vec::new() };
        }

        let samples_per_pixel = (total_frames as f64 / pixels_width as f64).ceil() as usize;
        let num_points = total_frames.div_ceil(samples_per_pixel);

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

    /// Get cached level closest to desired width
    pub fn get_level(&self, desired_width: usize) -> Option<WaveformLevel> {
        if desired_width == 0 {
            return None;
        }

        {
            let levels = self.levels.read();

            if let Some(level) = levels
                .iter()
                .find(|level| level.requested_width >= desired_width)
            {
                return Some(level.level.clone());
            }

            if let Some(level) = levels.last() {
                return Some(level.level.clone());
            }
        }

        let generated = self.generate(desired_width);
        let mut levels = self.levels.write();
        levels.push(CachedWaveformLevel {
            requested_width: desired_width,
            level: generated.clone(),
        });
        levels.sort_by_key(|level| level.requested_width);
        Some(generated)
    }
}
