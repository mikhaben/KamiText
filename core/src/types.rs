//! Public value types for the kamitext contract.
//!
//! All coordinates are UTF-8 byte offsets unless the field name says `utf16`.
//! Ranges are half-open `[start, end)`.

use bitflags::bitflags;

/// Half-open byte range into the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

impl ByteRange {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Do these two half-open ranges share at least one byte?
    ///
    /// A zero-width range touches `[a, b)` when `a <= p < b`.
    pub fn intersects(self, other: ByteRange) -> bool {
        if self.is_empty() {
            other.start <= self.start && self.start < other.end
        } else if other.is_empty() {
            self.start <= other.start && other.start < self.end
        } else {
            self.start < other.end && other.start < self.end
        }
    }
}

/// Half-open UTF-16 code-unit range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Utf16Range {
    pub start: u32,
    pub end: u32,
}

bitflags! {
    /// Style-kind set. Stable numeric ids.
    ///
    /// `BODY` is the base: a plain-text segment has kinds == `BODY` and no other
    /// bit. When any other kind applies, `BODY` is not set (it is the empty base).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Kind: u64 {
        const BODY           = 1 << 0;
        const HEADING1       = 1 << 1;
        const HEADING2       = 1 << 2;
        const HEADING3       = 1 << 3;
        const HEADING4       = 1 << 4;
        const HEADING5       = 1 << 5;
        const HEADING6       = 1 << 6;
        const STRONG         = 1 << 7;
        const EMPHASIS       = 1 << 8;
        const STRIKETHROUGH  = 1 << 9;
        const CODE_SPAN      = 1 << 10;
        const CODE_BLOCK     = 1 << 11;
        const FENCE_INFO     = 1 << 12;
        const BLOCKQUOTE     = 1 << 13;
        const LIST_BULLET    = 1 << 14;
        const LIST_ORDINAL   = 1 << 15;
        const TASK_MARKER    = 1 << 16;
        const LINK           = 1 << 17;
        const IMAGE          = 1 << 18;
        const TABLE          = 1 << 19;
        const THEMATIC_BREAK = 1 << 20;
        const MARKER         = 1 << 21;
        const HTML_RAW       = 1 << 22;
    }
}

impl Kind {
    /// The heading kind for an ATX/setext level (1..=6), clamped to 6.
    pub fn heading(level: u8) -> Kind {
        match level.clamp(1, 6) {
            1 => Kind::HEADING1,
            2 => Kind::HEADING2,
            3 => Kind::HEADING3,
            4 => Kind::HEADING4,
            5 => Kind::HEADING5,
            _ => Kind::HEADING6,
        }
    }
}

bitflags! {
    /// Toggleable GFM extensions. Default: all on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Extensions: u32 {
        const TABLES        = 1 << 0;
        const TASK_LISTS    = 1 << 1;
        const STRIKETHROUGH = 1 << 2;
    }
}

impl Default for Extensions {
    fn default() -> Self {
        Extensions::all()
    }
}

/// Reveal policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RevealMode {
    /// Reader mode: everything concealed.
    None,
    /// Default: reveal markers of spans intersecting the active physical line(s).
    #[default]
    Line,
    /// Reveal the whole containing block (reserved).
    Block,
    /// Per-element reveal (FFI value 3): `Inline`-scoped markers activate by
    /// selection overlap with their own owning element; `Block`-scoped
    /// markers stay line-scoped.
    Element,
}

/// Whether a marker conceals per the line/block it sits on (`Block`) or per
/// its own owning element under `RevealMode::Element` (`Inline`). Assigned
/// at the `push_marker` call site in parse.rs — the site that paints a
/// marker is the only place that reliably knows which it is; `kind` bits
/// alone don't disambiguate (a heading's `#` and a paragraph's `**` share
/// the bare `MARKER` bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerScope {
    Block,
    Inline,
}

/// Engine configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineOptions {
    pub extensions: Extensions,
    pub reveal: RevealMode,
}

/// A maximal run of text with a constant kind set and conceal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub range: ByteRange,
    pub utf16: Utf16Range,
    pub kinds: Kind,
    pub concealed: bool,
}

/// The semantic classification of an [`Element`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Task { checked: bool },
    Link { dest: ByteRange },
    Image { src: ByteRange },
    Fence { info: ByteRange },
}

/// An interactive semantic object. `id` is stable within a parse
/// generation (reassigned every reparse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Element {
    pub id: u32,
    pub range: ByteRange,
    pub kind: ElementKind,
}

/// The set of byte ranges whose segments changed after a mutating call.
/// Sorted, coalesced, non-overlapping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patch {
    pub dirty: Vec<ByteRange>,
}

/// A suggested text mutation the adapter applies via `apply_edit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPlan {
    /// Applied back-to-front (descending start offset).
    pub edits: Vec<(ByteRange, String)>,
    /// Byte offset the caret should occupy after application.
    pub caret: u32,
}

/// Error surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KamiError {
    /// Out-of-bounds or scalar-splitting range on a mutating call.
    InvalidRange,
}

impl core::fmt::Display for KamiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            KamiError::InvalidRange => write!(f, "invalid range: out of bounds or splits a scalar"),
        }
    }
}

impl std::error::Error for KamiError {}
