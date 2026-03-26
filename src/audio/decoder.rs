//! Audio decoder using Symphonia
//!
//! Handles loading and decoding various audio formats (MP3, FLAC, WAV, OGG, AAC)

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Represents decoded audio data
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Interleaved samples (left, right, left, right, ...)
    pub samples: Vec<f32>,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u16,
    /// Total duration
    pub duration: Duration,
}

/// Audio decoder using Symphonia
pub struct AudioDecoder;

impl AudioDecoder {
    /// Decode an audio file from the given path
    pub fn decode_file<P: AsRef<Path>>(path: P) -> Result<DecodedAudio> {
        let path = path.as_ref();

        // Open the file
        let file = File::open(path).with_context(|| format!("Failed to open file: {:?}", path))?;

        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // Create a hint with the file extension
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        // Use the default options for metadata, format, and decoder
        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        let decoder_opts = DecoderOptions::default();

        // Probe the media source stream
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .with_context(|| "Failed to probe audio file")?;

        let mut format = probed.format;

        // Find the first audio track
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow::anyhow!("No audio track found"))?;

        let track_id = track.id;
        let codec_params = &track.codec_params;

        // Create a decoder for the audio track
        let mut decoder = symphonia::default::get_codecs()
            .make(codec_params, &decoder_opts)
            .with_context(|| "Failed to create decoder")?;

        // Get audio parameters
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| anyhow::anyhow!("Unknown sample rate"))?;
        let channels = codec_params
            .channels
            .ok_or_else(|| anyhow::anyhow!("Unknown channel count"))?
            .count() as u16;

        // Decode all frames
        let mut all_samples = Vec::new();
        let mut frame_count = 0usize;
        let mut sample_buf: Option<SampleBuffer<f32>> = None;

        loop {
            match format.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => {
                            // Create or resize sample buffer if needed
                            if sample_buf.is_none()
                                || sample_buf.as_ref().unwrap().capacity()
                                    < decoded.capacity() as usize
                            {
                                sample_buf = Some(SampleBuffer::<f32>::new(
                                    decoded.capacity() as u64,
                                    *decoded.spec(),
                                ));
                            }

                            // Copy interleaved samples
                            if let Some(buf) = &mut sample_buf {
                                buf.copy_interleaved_ref(decoded);
                                let samples = buf.samples();
                                all_samples.extend_from_slice(samples);
                                frame_count += samples.len() / channels as usize;
                            }
                        }
                        Err(SymphoniaError::DecodeError(_)) => {
                            // Skip decode errors
                            continue;
                        }
                        Err(e) => bail!("Decode error: {}", e),
                    }
                }
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // End of stream
                    break;
                }
                Err(e) => bail!("Packet error: {}", e),
            }
        }

        // Calculate duration
        let duration = Duration::from_secs_f64(frame_count as f64 / sample_rate as f64);

        Ok(DecodedAudio {
            samples: all_samples,
            sample_rate,
            channels,
            duration,
        })
    }

    /// Check if a file format is supported
    pub fn is_supported<P: AsRef<Path>>(path: P) -> bool {
        let ext = path
            .as_ref()
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());

        matches!(
            ext.as_deref(),
            Some("mp3" | "flac" | "wav" | "ogg" | "aac" | "m4a")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported() {
        assert!(AudioDecoder::is_supported("test.mp3"));
        assert!(AudioDecoder::is_supported("test.FLAC"));
        assert!(AudioDecoder::is_supported("test.wav"));
        assert!(AudioDecoder::is_supported("test.ogg"));
        assert!(!AudioDecoder::is_supported("test.txt"));
    }
}
