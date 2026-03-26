//! UI Widgets module - custom egui widgets

pub mod loop_control;
pub mod playback_controls;
pub mod speed_control;
pub mod time_display;
pub mod waveform_display;

pub use loop_control::LoopControl;
pub use playback_controls::PlaybackControls;
pub use speed_control::SpeedControl;
pub use time_display::TimeDisplay;
pub use waveform_display::WaveformDisplay;
