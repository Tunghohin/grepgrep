//! Waveform and spectrogram data generation for visualization.

use parking_lot::RwLock;
use realfft::RealFftPlanner;
use std::sync::Arc;

const MAX_CACHED_WINDOWS: usize = 8;
const FFT_WINDOW_SIZE: usize = 16_384;
const FFT_HOP_SIZE: usize = 2_048;
const MIN_MIDI_NOTE: i32 = 21;
const MAX_MIDI_NOTE: i32 = 108;
const MIN_SPECTROGRAM_DB: f32 = -90.0;
const SPECTROGRAM_CONTRAST_FLOOR_PERCENTILE: f32 = 0.70;
const SPECTROGRAM_CONTRAST_GAMMA: f32 = 0.6;

/// Represents a single point in the waveform display.
#[derive(Debug, Clone, Copy)]
pub struct WaveformPoint {
    /// Minimum sample value in this region.
    pub min: f32,
    /// Maximum sample value in this region.
    pub max: f32,
}

/// Waveform data at a specific resolution.
#[derive(Debug, Clone)]
pub struct WaveformLevel {
    /// Points representing the waveform.
    pub points: Vec<WaveformPoint>,
}

/// Continuous pitch axis used by the spectrogram view.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpectrogramPitchAxis {
    /// Lowest fully supported note on the axis.
    pub min_midi_note: i32,
    /// Highest note that still has at least partial coverage on the axis.
    pub max_midi_note: i32,
    /// Lower cent bound of the axis.
    pub min_pitch_cents: f32,
    /// Upper cent bound of the axis.
    pub max_pitch_cents: f32,
}

impl SpectrogramPitchAxis {
    /// Total cent span covered by the axis.
    pub fn pitch_span_cents(&self) -> f32 {
        (self.max_pitch_cents - self.min_pitch_cents).max(1.0)
    }
}

/// Rasterized spectrogram data for the current viewport.
#[derive(Debug, Clone)]
pub struct SpectrogramView {
    /// Pixel width of the generated view.
    pub width: usize,
    /// Pixel height of the generated view.
    pub height: usize,
    /// Heatmap intensity values in row-major order.
    pub intensities: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CachedWaveformWindow {
    start_frame: usize,
    end_frame: usize,
    requested_width: usize,
    level: WaveformLevel,
}

#[derive(Debug, Clone)]
struct CachedSpectrogramWindow {
    start_frame: usize,
    end_frame: usize,
    requested_width: usize,
    requested_height: usize,
    view: SpectrogramView,
}

#[derive(Debug, Clone, Default)]
struct SpectrogramData {
    hop_size: usize,
    axis: SpectrogramPitchAxis,
    fft_bin_count: usize,
    time_bins: usize,
    bin_centers_cents: Vec<f32>,
    magnitudes: Vec<u8>,
}

impl SpectrogramData {
    fn from_samples(samples: &Arc<[f32]>, channels: u16, sample_rate: u32) -> Self {
        let channels = channels.max(1) as usize;
        let total_frames = samples.len() / channels;
        if total_frames == 0 || sample_rate == 0 {
            return Self::default();
        }

        let min_freq = midi_to_frequency(MIN_MIDI_NOTE as f32);
        let max_freq = midi_to_frequency(MAX_MIDI_NOTE as f32).min(sample_rate as f32 / 2.0);
        if max_freq <= min_freq {
            return Self::default();
        }

        let axis = SpectrogramPitchAxis {
            min_midi_note: MIN_MIDI_NOTE,
            max_midi_note: (frequency_to_midi(max_freq) * 100.0).floor() as i32 / 100,
            min_pitch_cents: MIN_MIDI_NOTE as f32 * 100.0,
            max_pitch_cents: (frequency_to_midi(max_freq) * 100.0)
                .min(MAX_MIDI_NOTE as f32 * 100.0),
        };
        if axis.max_pitch_cents <= axis.min_pitch_cents {
            return Self::default();
        }

        let bin_hz = sample_rate as f32 / FFT_WINDOW_SIZE as f32;
        let low_bin = ((min_freq / bin_hz).floor() as usize).max(1);
        let high_bin = ((max_freq / bin_hz).ceil() as usize)
            .min(FFT_WINDOW_SIZE / 2)
            .max(low_bin);
        let fft_bin_count = high_bin - low_bin + 1;
        if fft_bin_count == 0 {
            return Self::default();
        }

        let time_bins = ceil_div(total_frames.max(1), FFT_HOP_SIZE).max(1);
        let mut magnitudes = vec![0_u8; time_bins * fft_bin_count];
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_WINDOW_SIZE);
        let mut input = fft.make_input_vec();
        let mut spectrum = fft.make_output_vec();
        let window = hann_window(FFT_WINDOW_SIZE);
        let window_gain = window.iter().copied().sum::<f32>().max(1.0);
        let bin_centers_cents = (low_bin..=high_bin)
            .map(|bin| frequency_to_midi(bin as f32 * bin_hz) * 100.0)
            .collect::<Vec<_>>();

        for time_bin in 0..time_bins {
            let start_frame = time_bin * FFT_HOP_SIZE;
            input.fill(0.0);

            for (offset, sample_out) in input.iter_mut().enumerate() {
                let frame = start_frame + offset;
                if frame >= total_frames {
                    break;
                }

                let mut mono = 0.0;
                for channel in 0..channels {
                    mono += samples[frame * channels + channel];
                }
                mono /= channels as f32;
                *sample_out = mono * window[offset];
            }

            if fft.process(&mut input, &mut spectrum).is_err() {
                return Self::default();
            }

            let row = &mut magnitudes[time_bin * fft_bin_count..(time_bin + 1) * fft_bin_count];
            for (row_index, bin) in (low_bin..=high_bin).enumerate() {
                let amplitude = 2.0_f32 * spectrum[bin].norm() / window_gain;
                row[row_index] = normalize_amplitude_to_u8(amplitude);
            }
        }

        Self {
            hop_size: FFT_HOP_SIZE,
            axis,
            fft_bin_count,
            time_bins,
            bin_centers_cents,
            magnitudes,
        }
    }

    fn is_empty(&self) -> bool {
        self.fft_bin_count == 0
            || self.time_bins == 0
            || self.bin_centers_cents.is_empty()
            || self.magnitudes.is_empty()
    }
}

/// Multi-resolution waveform cache.
pub struct WaveformGenerator {
    /// Decoded audio samples.
    samples: Arc<[f32]>,
    /// Number of channels.
    channels: u16,
    /// Total number of frames in the source audio.
    total_frames: usize,
    /// Cached waveform windows for recent viewports.
    windows: RwLock<Vec<CachedWaveformWindow>>,
    /// Cached spectrogram heatmaps for recent viewports.
    spectrogram_windows: RwLock<Vec<CachedSpectrogramWindow>>,
    /// High-resolution spectrogram data used by the heatmap view.
    spectrogram: SpectrogramData,
}

impl WaveformGenerator {
    /// Create a new waveform generator.
    pub fn new<S>(samples: S, channels: u16, sample_rate: u32) -> Self
    where
        S: Into<Arc<[f32]>>,
    {
        let samples = samples.into();
        let channels = channels.max(1);
        let spectrogram = SpectrogramData::from_samples(&samples, channels, sample_rate);

        Self {
            total_frames: samples.len() / channels as usize,
            samples,
            channels,
            windows: RwLock::new(Vec::new()),
            spectrogram_windows: RwLock::new(Vec::new()),
            spectrogram,
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
        let channels = self.channels.max(1) as usize;
        let mut points = Vec::with_capacity(num_points);

        for i in 0..num_points {
            let start = start_frame + i * window_frames / num_points;
            let mut end = start_frame + (i + 1) * window_frames / num_points;
            if end <= start {
                end = (start + 1).min(end_frame);
            }

            let mut min = f32::MAX;
            let mut max = f32::MIN;

            for frame in start..end {
                let mut mono = 0.0;
                for channel in 0..channels {
                    mono += samples[frame * channels + channel];
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

        if windows.len() > MAX_CACHED_WINDOWS {
            let excess = windows.len() - MAX_CACHED_WINDOWS;
            windows.drain(..excess);
        }

        Some(generated)
    }

    /// Get the continuous pitch axis used by the spectrogram.
    pub fn spectrogram_pitch_axis(&self) -> Option<SpectrogramPitchAxis> {
        (!self.spectrogram.is_empty()).then_some(self.spectrogram.axis)
    }

    /// Get cached spectrogram data for the current visible frame window.
    pub fn get_spectrogram_window(
        &self,
        start_frame: usize,
        end_frame: usize,
        desired_width: usize,
        desired_height: usize,
    ) -> Option<SpectrogramView> {
        if desired_width == 0 || desired_height == 0 || self.total_frames == 0 {
            return None;
        }

        if self.spectrogram.is_empty() {
            return None;
        }

        let start_frame = start_frame.min(self.total_frames);
        let end_frame = end_frame.clamp(start_frame.saturating_add(1), self.total_frames);

        {
            let windows = self.spectrogram_windows.read();
            if let Some(window) = windows.iter().find(|window| {
                window.start_frame == start_frame
                    && window.end_frame == end_frame
                    && window.requested_width == desired_width
                    && window.requested_height == desired_height
            }) {
                return Some(window.view.clone());
            }
        }

        let generated =
            self.generate_spectrogram_window(start_frame, end_frame, desired_width, desired_height);
        let mut windows = self.spectrogram_windows.write();
        windows.push(CachedSpectrogramWindow {
            start_frame,
            end_frame,
            requested_width: desired_width,
            requested_height: desired_height,
            view: generated.clone(),
        });

        if windows.len() > MAX_CACHED_WINDOWS {
            let excess = windows.len() - MAX_CACHED_WINDOWS;
            windows.drain(..excess);
        }

        Some(generated)
    }

    fn generate_spectrogram_window(
        &self,
        start_frame: usize,
        end_frame: usize,
        desired_width: usize,
        desired_height: usize,
    ) -> SpectrogramView {
        let spectrogram = &self.spectrogram;
        if spectrogram.is_empty() {
            return SpectrogramView {
                width: desired_width,
                height: desired_height,
                intensities: Vec::new(),
            };
        }

        let start_bin = (start_frame / spectrogram.hop_size).min(spectrogram.time_bins - 1);
        let end_bin = ceil_div(end_frame, spectrogram.hop_size)
            .clamp(start_bin.saturating_add(1), spectrogram.time_bins);
        let visible_bins = end_bin - start_bin;
        let axis = spectrogram.axis;
        let pitch_span = axis.pitch_span_cents();
        let mut intensities = vec![0_u8; desired_width * desired_height];
        let mut column_maxima = vec![0_u8; spectrogram.fft_bin_count];

        for x in 0..desired_width {
            column_maxima.fill(0);
            let time_start = start_bin + x * visible_bins / desired_width;
            let mut time_end = start_bin + (x + 1) * visible_bins / desired_width;
            if time_end <= time_start {
                time_end = (time_start + 1).min(end_bin);
            }

            for time_bin in time_start..time_end {
                let row = &spectrogram.magnitudes[time_bin * spectrogram.fft_bin_count
                    ..(time_bin + 1) * spectrogram.fft_bin_count];
                for (slot, &value) in column_maxima.iter_mut().zip(row.iter()) {
                    *slot = (*slot).max(value);
                }
            }

            for (bin_index, &value) in column_maxima.iter().enumerate() {
                if value == 0 {
                    continue;
                }

                let cent = spectrogram.bin_centers_cents[bin_index]
                    .clamp(axis.min_pitch_cents, axis.max_pitch_cents);
                let y = if desired_height <= 1 {
                    0.0
                } else {
                    (axis.max_pitch_cents - cent) / pitch_span * (desired_height - 1) as f32
                };
                splat_spectrogram_intensity(
                    &mut intensities,
                    desired_width,
                    desired_height,
                    x,
                    y,
                    value,
                );
            }
        }

        enhance_spectrogram_contrast(&mut intensities);

        SpectrogramView {
            width: desired_width,
            height: desired_height,
            intensities,
        }
    }
}

fn splat_spectrogram_intensity(
    intensities: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: f32,
    value: u8,
) {
    if width == 0 || height == 0 {
        return;
    }

    if height == 1 {
        intensities[x] = intensities[x].max(value);
        return;
    }

    let base_row = y.floor().clamp(0.0, (height - 1) as f32) as usize;
    let frac = (y - base_row as f32).clamp(0.0, 1.0);
    let primary = ((value as f32) * (1.0 - frac)).round() as u8;
    let secondary = ((value as f32) * frac).round() as u8;

    let primary_index = base_row * width + x;
    intensities[primary_index] = intensities[primary_index].max(primary);

    if base_row + 1 < height {
        let secondary_index = (base_row + 1) * width + x;
        intensities[secondary_index] = intensities[secondary_index].max(secondary);
    }
}

fn normalize_amplitude_to_u8(amplitude: f32) -> u8 {
    let db = 20.0_f32 * amplitude.max(1.0e-5_f32).log10();
    let normalized = ((db - MIN_SPECTROGRAM_DB) / -MIN_SPECTROGRAM_DB)
        .clamp(0.0, 1.0)
        .powf(0.85);
    (normalized * 255.0).round() as u8
}

fn enhance_spectrogram_contrast(intensities: &mut [u8]) {
    if intensities.is_empty() {
        return;
    }

    let mut histogram = [0_usize; 256];
    let mut nonzero_count = 0_usize;
    let mut max_value = 0_u8;

    for &value in intensities.iter() {
        if value > 0 {
            histogram[value as usize] += 1;
            nonzero_count += 1;
            max_value = max_value.max(value);
        }
    }

    if nonzero_count == 0 {
        return;
    }

    let floor = percentile_from_histogram(
        &histogram,
        nonzero_count,
        SPECTROGRAM_CONTRAST_FLOOR_PERCENTILE,
    )
    .max(1);
    let ceiling = max_value.max(floor.saturating_add(1));
    let range = ceiling.saturating_sub(floor).max(1) as f32;

    for value in intensities.iter_mut() {
        if *value <= floor {
            *value = 0;
            continue;
        }

        let normalized = ((*value - floor) as f32 / range).clamp(0.0, 1.0);
        let boosted = normalized.powf(SPECTROGRAM_CONTRAST_GAMMA);
        *value = (boosted * 255.0).round() as u8;
    }
}

fn percentile_from_histogram(histogram: &[usize; 256], total_count: usize, percentile: f32) -> u8 {
    if total_count == 0 {
        return 0;
    }

    let target = ((total_count.saturating_sub(1)) as f32 * percentile.clamp(0.0, 1.0)).round()
        as usize;
    let mut cumulative = 0_usize;

    for (value, &count) in histogram.iter().enumerate() {
        cumulative += count;
        if cumulative > target {
            return value as u8;
        }
    }

    255
}

fn ceil_div(value: usize, divisor: usize) -> usize {
    value.saturating_add(divisor.saturating_sub(1)) / divisor
}

fn hann_window(size: usize) -> Vec<f32> {
    let tau = std::f32::consts::TAU;
    (0..size)
        .map(|index| 0.5 - 0.5 * (tau * index as f32 / size as f32).cos())
        .collect()
}

fn frequency_to_midi(freq: f32) -> f32 {
    if freq <= 0.0 {
        return MIN_MIDI_NOTE as f32;
    }

    69.0 + 12.0 * (freq / 440.0).log2()
}

fn midi_to_frequency(midi: f32) -> f32 {
    440.0 * 2.0_f32.powf((midi - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

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

    #[test]
    fn spectrogram_window_generation_matches_requested_size() {
        let generator = WaveformGenerator::new(vec![0.0; 48_000], 1, 48_000);

        let view = generator
            .get_spectrogram_window(0, generator.frame_count(), 64, 48)
            .expect("spectrogram window should be generated");

        assert_eq!(view.width, 64);
        assert_eq!(view.height, 48);
        assert_eq!(view.intensities.len(), 64 * 48);
    }

    #[test]
    fn spectrogram_peak_is_within_five_cents_of_a4() {
        let sample_rate = 48_000;
        let samples = (0..sample_rate)
            .map(|frame| {
                let phase = TAU * 440.0 * frame as f32 / sample_rate as f32;
                0.8 * phase.sin()
            })
            .collect::<Vec<_>>();
        let generator = WaveformGenerator::new(samples, 1, sample_rate as u32);
        let axis = generator
            .spectrogram_pitch_axis()
            .expect("spectrogram should expose a pitch axis");
        let height = axis.pitch_span_cents().ceil() as usize + 1;
        let view = generator
            .get_spectrogram_window(0, generator.frame_count(), 1, height)
            .expect("spectrogram window should be generated");

        let (peak_row, _) = view
            .intensities
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|(_, intensity)| *intensity)
            .expect("peak row should exist");
        let peak_cent = row_to_cent(axis, peak_row, height);

        assert!((peak_cent - 6_900.0).abs() <= 5.0, "peak_cent={peak_cent}");
    }

    #[test]
    fn spectrogram_axis_clips_to_nyquist_when_needed() {
        let generator = WaveformGenerator::new(vec![0.0; 8_000], 1, 8_000);
        let axis = generator
            .spectrogram_pitch_axis()
            .expect("spectrogram should expose a pitch axis");

        assert!(axis.max_pitch_cents < MAX_MIDI_NOTE as f32 * 100.0);
        assert_eq!(axis.min_midi_note, MIN_MIDI_NOTE);
    }

    #[test]
    fn spectrogram_contrast_stretch_suppresses_floor_and_boosts_peaks() {
        let mut intensities = vec![
            0, 0, 12, 14, 16, 18, 20, 20, 22, 24, 26, 160, 180, 220, 255,
        ];

        enhance_spectrogram_contrast(&mut intensities);

        assert_eq!(intensities[0], 0);
        assert_eq!(intensities[2], 0);
        assert!(intensities[11] > 180, "expected strong bins to be boosted");
        assert_eq!(intensities[14], 255);
    }

    fn row_to_cent(axis: SpectrogramPitchAxis, row: usize, height: usize) -> f32 {
        if height <= 1 {
            return axis.max_pitch_cents;
        }

        axis.max_pitch_cents - row as f32 / (height - 1) as f32 * axis.pitch_span_cents()
    }
}
