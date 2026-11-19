//! Audio processing core of SCh Tomy Audio, the sermon podcast tool.
//!
//! Everything here is independent of the GUI so it can be tested with plain
//! `cargo test` on every platform.

pub mod error;
pub mod ffmpeg;
pub mod filters;
pub mod history;
pub mod naming;
pub mod parse;
pub mod pcm;
pub mod pipeline;
pub mod probe;
pub mod settings;
pub mod speakers;
pub mod tags;
pub mod teaser;
pub mod vad;
pub mod youtube;

pub use error::{CoreError, ErrorKind, Result};
pub use ffmpeg::{CancelToken, Tools};
