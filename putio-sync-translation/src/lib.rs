//! # putio-sync-translation
//!
//! Subtitle translation (feature 026): the prompt and reply format, the chunk driver with its
//! retry and split rules, the OpenAI-compatible chat client, the callback translator's token
//! and body handling, and the checks that keep every cue's timing exactly as the source's.
//!
//! The pipeline crate schedules; this crate translates one subtitle at a time.

pub mod align;
pub mod callback;
pub mod chunk;
pub mod prompt;
pub mod source;
