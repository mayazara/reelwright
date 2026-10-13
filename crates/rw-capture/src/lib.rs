//! Screen, webcam and microphone recording.
//!
//! Pictures come from FFmpeg's capture devices (see [`devices`]), sound
//! from the system's sound inputs (see [`sources`]). A [`recorder::Recorder`]
//! writes each picture source to its own crash-safe file, all stamped by
//! one pause-aware clock, with the sound kept in step by [`sync`].

pub mod clock;
pub mod devices;
pub mod recorder;
pub mod sources;
pub mod sync;

pub use recorder::{FileKind, Options, RecordedFile, Recorder, Sources};
