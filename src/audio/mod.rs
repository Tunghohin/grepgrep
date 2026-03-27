//! Audio module - handles audio decoding and playback
//!
//! This module provides:
//! - Audio file decoding using Symphonia
//! - Audio output stream management using cpal
//! - Audio buffering for smooth playback

pub mod buffer;
pub mod decoder;
pub mod playback;

pub use buffer::{AudioBuffer, AudioChannelMode};
pub use decoder::AudioDecoder;
pub use playback::AudioPlayer;
