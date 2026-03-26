//! State module - application state management
//!
//! This module provides:
//! - Global application state
//! - Audio playback state
//! - UI state

pub mod app_state;

pub use app_state::{AppState, LoopRegion, TimelineTag};
