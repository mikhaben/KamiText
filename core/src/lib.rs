//! kamitext — a portable Markdown editor engine.
//!
//! Given a document, an edit and a caret, it decides *what should look like
//! what*: style segments, conceal state, dirty patches and typing behaviors.
//! It never paints pixels, never touches files, never renders HTML.
//!
//! The behavioral contract: the invariants in AGENTS.md plus the
//! conformance fixtures in `fixtures/`.

#![deny(unsafe_code)]

mod analysis;
mod behaviors;
mod conceal;
mod document;
mod engine;
mod offsets;
mod parse;
mod patch;
pub mod types;

#[allow(unsafe_code)]
pub mod ffi;

pub use engine::Engine;
pub use types::{
    ByteRange, EditPlan, Element, ElementKind, EngineOptions, Extensions, KamiError, Kind, Patch,
    RevealMode, Segment, Utf16Range,
};
