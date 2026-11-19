//! Audio processing core of the SCh Tomy podcast tool.
//!
//! Everything here is independent of the GUI so it can be tested with plain
//! `cargo test` on every platform.

pub mod error;
pub mod ffmpeg;
pub mod filters;
pub mod naming;
pub mod parse;
pub mod pipeline;
pub mod probe;
pub mod settings;
pub mod tags;

pub use error::{CoreError, ErrorKind, Result};
pub use ffmpeg::{CancelToken, Tools};
