//! pulldown-cmark walk → style paints, marker paints, elements.
//!
//! Marker derivation strategy (validated against a live probe of
//! pulldown-cmark 0.13 event ranges):
//! - emphasis / strong / strikethrough / link / image: **gap technique** —
//!   the node's range minus its direct children's span yields the leading and
//!   trailing delimiter runs exactly.
//! - headings, blockquotes, fences, list bullets, code spans: **targeted
//!   scans** within the node's own range (the gap trick is insufficient:
//!   heading ranges include the trailing newline, blockquote `>` prefixes
//!   repeat per line, fences carry an info string).
//!
//! Composition rule: an *inline* node's delimiters carry the *enclosing*
//! kinds plus `MARKER`, never the node's own kind — each node paints its kind
//! over (own range minus own markers), and markers are separate paints
//! unioned during flattening. Three block constructs deliberately break this
//! and union their own kind onto the marker (blockquote `>` prefixes, table
//! source, thematic breaks): their hosts key block decorations off the
//! marker's kind bits.

use crate::types::{ByteRange, Element, ElementKind, Extensions, Kind, MarkerScope};
use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use unicase::UniCase;

/// A content-kind paint: adds `kind` to every byte in `range`. Never concealed.
#[derive(Debug, Clone, Copy)]
pub struct Paint {
    pub range: ByteRange,
    pub kind: Kind,
}

/// A marker paint: adds `kind` (MARKER / FENCE_INFO / TASK_MARKER) and makes
/// the bytes concealable. `owner` is the syntactic span the marker belongs to;
/// the reveal policy tests `owner` against the reveal region.
/// `scope` is assigned by the call site that paints this marker — it is not
/// recoverable later from `kind` alone.
#[derive(Debug, Clone, Copy)]
pub struct MarkerPaint {
    pub range: ByteRange,
    pub kind: Kind,
    pub owner: ByteRange,
    pub scope: MarkerScope,
}

/// The `[ ]` box of a task item, for `toggle_task_plan`.
#[derive(Debug, Clone, Copy)]
pub struct TaskBox {
    pub item: ByteRange,
    pub boxx: ByteRange,
    pub checked: bool,
}

/// Everything one parse produces. Buffers are reused across parses (cleared,
/// not reallocated).
#[derive(Default)]
pub struct ParseOut {
    pub paints: Vec<Paint>,
    pub markers: Vec<MarkerPaint>,
    pub elements: Vec<Element>,
    /// Running prefix-max of `elements[i].range.end`, in the same order as
    /// `elements` — powers `Engine::elements_in`'s binary search.
    pub elements_max_end: Vec<u32>,
    pub task_boxes: Vec<TaskBox>,
    /// Code blocks + HTML blocks: regions where typing behaviors must not
    /// apply lexical list/quote continuation.
    pub verbatim_blocks: Vec<ByteRange>,
    /// Top-level block ranges, for `RevealMode::Block`.
    pub blocks: Vec<ByteRange>,
}

impl ParseOut {
    pub fn clear(&mut self) {
        self.paints.clear();
        self.markers.clear();
        self.elements.clear();
        self.elements_max_end.clear();
        self.task_boxes.clear();
        self.verbatim_blocks.clear();
        self.blocks.clear();
    }
}

enum NodeKind {
    Heading(u8),
    BlockQuote,
    FencedCode,
    IndentedCode,
    Item,
    Emphasis,
    Strong,
    Strikethrough,
    Link { reference: Option<String> },
    WikiLink { has_pothole: bool, dest_len: u32 },
    Image { reference: Option<String> },
    WikiImage { has_pothole: bool, dest_len: u32 },
    Table,
    HtmlBlock,
    Other,
}

struct Frame {
    node: NodeKind,
    /// Range from the Start event (== the node's full range).
    range: ByteRange,
    /// Span of direct children: u32::MAX start when none seen yet.
    first_child: u32,
    last_child_end: u32,
    /// Number of enclosing blockquotes (for per-line `>` prefix skipping).
    quote_depth: u32,
    /// Inside a table: inline markers/elements are suppressed there — markers
    /// must be disjoint, and the whole-table Block marker owns conceal, so a
    /// revealed table shows its full raw source.
    in_table: bool,
    /// Task box captured from a TaskListMarker event (items only).
    task: Option<(ByteRange, bool)>,
}

impl Frame {
    /// Span of direct children, clamped to the node's own range. pulldown
    /// 0.13's empty-alias wikilink (`[[a|]]` + trailing inline) re-emits the
    /// paragraph's trailing events inside the still-open link, inflating
    /// `last_child_end` past the node's end — unclamped, that yields a
    /// reversed trail marker that corrupts the sweep-line. A span that
    /// collapses under the clamp is reported as body-less (`None`).
    fn child_span(&self) -> Option<ByteRange> {
        if self.first_child == u32::MAX {
            return None;
        }
        let start = self.first_child.max(self.range.start);
        let end = self.last_child_end.min(self.range.end);
        if start >= end {
            None
        } else {
            Some(ByteRange::new(start, end))
        }
    }
}

/// Parses `text` into `out`, degrading to unstyled plain text if the pulldown
/// walk panics.
///
/// pulldown-cmark 0.13.4 panics on inputs a user reaches by typing — the
/// `![[…]]` embed shapes `![[]a]()]]`, `![[]|]()]]`, `![[] ]()]]`, `![[]*]()]]`
/// drive `handle_wikilink` into a reversed slice (its own start past its end).
/// Unguarded, the unwind reaches the C ABI, which poisons the engine: one
/// keystroke ends the editing session until the app restarts.
///
/// REMOVE THIS GUARD once the dependency reaches a release containing
/// pulldown-cmark `ebf31da8` ("Fix subtract-overflow panic in handle_wikilink
/// on malformed input", PR #1111 / issue #1108, merged 2026-07-08), which adds
/// the `end_ix <= start_ix` bail this works around. It is unreleased as of
/// 0.13.4 (2026-05-20) — the newest release predates the fix, which is why the
/// guard still earns its place. Deleting it will re-expose the four shapes
/// above, so bump the dependency and confirm `pulldown_panic_inputs_degrade_to_plain_text`
/// fails before removing anything.
///
/// Recovery is sound here because `parse` is a pure function of
/// `(text, extensions)` and Rust unwinding is memory-safe: a half-filled
/// `ParseOut` is STALE, not corrupt, so clearing it restores a fully defined
/// state — bit-for-bit the arena a markup-free document produces, which
/// downstream already covers (`flatten` emits one plain covering segment).
/// Poisoning stays the right answer where post-panic state is *unknown*; this
/// state is known, so one document losing its styling beats losing the session.
///
/// DEBUG BUILDS RE-PANIC, deliberately, so kami's own parser bugs keep failing
/// loudly instead of degrading into plain text. Mind the reach: only `cargo
/// test` and `cargo run` are debug here — `build-xcframework.sh` compiles
/// `--release`, so every Swift host (both apps and `KamiDemoMac`) links a
/// release core and recovers quietly even from a Swift Debug build. The Rust
/// suite is the gate that keeps this honest. Without the re-panic the guard
/// would be a trap swallowing every future defect in the walk — a worse bug
/// than the one it fixes.
pub fn parse(text: &str, extensions: Extensions, out: &mut ParseOut) {
    // `&mut ParseOut` is not `UnwindSafe`; asserting it is sound *because* the
    // catch arm resets the arena, so no caller can observe a partial parse.
    // The debug arm re-panics WITHOUT clearing, so an embedder that catches
    // that unwind itself would observe a half-filled arena. The FFI never does
    // — it poisons the cell or drops the engine — but a future Rust embedder
    // must reseed rather than reuse the `Engine`.
    match catch_unwind(AssertUnwindSafe(|| walk(text, extensions, out))) {
        Ok(()) => {}
        Err(payload) => {
            if cfg!(debug_assertions) {
                resume_unwind(payload);
            }
            out.clear();
        }
    }
}

/// The pulldown walk itself — everything `parse`'s guard covers, and nothing
/// else. Kept separate so the catch cannot creep outward over engine work that
/// has no plain-text fallback.
fn walk(text: &str, extensions: Extensions, out: &mut ParseOut) {
    out.clear();
    let mut opts = Options::empty();
    if extensions.contains(Extensions::TABLES) {
        opts.insert(Options::ENABLE_TABLES);
    }
    if extensions.contains(Extensions::TASK_LISTS) {
        opts.insert(Options::ENABLE_TASKLISTS);
    }
    if extensions.contains(Extensions::STRIKETHROUGH) {
        opts.insert(Options::ENABLE_STRIKETHROUGH);
    }
    if extensions.contains(Extensions::WIKILINKS) {
        opts.insert(Options::ENABLE_WIKILINKS);
    }

    let mut stack: Vec<Frame> = Vec::new();
    let mut quote_depth = 0u32;
    let mut table_depth = 0u32;
    // Depth of a spurious subtree currently being skipped (see below).
    let mut skip_depth = 0u32;

    // Reference definitions are resolved by pulldown's eager first pass and
    // exposed on the OffsetIter before we consume it. Snapshot them owned:
    // `def_spans` (span-sorted) drives def-line concealment (§2.3), and
    // `def_urls` (label-sorted) resolves link dests by binary search (§2.1) —
    // O((links+defs)·log defs), keeping the apply_edit p50 gate safe on
    // def-heavy documents. Labels fold with `UniCase`, the SAME type pulldown's
    // own RefDefs map keys on, so our lookup can never disagree with pulldown's
    // resolution (an ASCII fold would leave a resolved non-ASCII link with an
    // empty dest). Both sorts are total orders over unique keys (one def per
    // label; spans can't collide), so output stays deterministic.
    let iter = Parser::new_ext(text, opts).into_offset_iter();
    let mut def_spans: Vec<ByteRange> = Vec::new();
    let mut def_urls: Vec<(UniCase<String>, ByteRange)> = Vec::new();
    for (label, def) in iter.reference_definitions().iter() {
        let span = ByteRange::new(def.span.start as u32, def.span.end as u32);
        def_spans.push(span);
        def_urls.push((UniCase::new(label.to_string()), scan_def_url(text, span)));
    }
    def_spans.sort_by_key(|s| s.start);
    def_urls.sort_by(|a, b| a.0.cmp(&b.0));

    for (event, range) in iter {
        let r = ByteRange::new(range.start as u32, range.end as u32);
        // pulldown 0.13's empty-alias wikilink (`[[a|]]` + trailing inline)
        // re-emits the rest of the paragraph *inside* the still-open link —
        // duplicating text, markers, and whole sibling nodes (probed; see
        // the zz golden tests). A legitimate child never starts at or past
        // its parent's end, so any such non-End event is spurious: drop it,
        // and for a Start, its entire subtree.
        if skip_depth > 0 {
            match event {
                Event::Start(_) => skip_depth += 1,
                Event::End(_) => skip_depth -= 1,
                _ => {}
            }
            continue;
        }
        if !matches!(event, Event::End(_))
            && let Some(top) = stack.last()
            && r.start >= top.range.end
        {
            if matches!(event, Event::Start(_)) {
                skip_depth = 1;
            }
            continue;
        }
        match event {
            Event::Start(tag) => {
                note_child(&mut stack, r);
                if stack.is_empty() {
                    out.blocks.push(r);
                }
                let node = match tag {
                    Tag::Heading { level, .. } => NodeKind::Heading(level as u8),
                    Tag::BlockQuote(_) => NodeKind::BlockQuote,
                    Tag::CodeBlock(CodeBlockKind::Fenced(_)) => NodeKind::FencedCode,
                    Tag::CodeBlock(CodeBlockKind::Indented) => NodeKind::IndentedCode,
                    Tag::Item => NodeKind::Item,
                    Tag::Emphasis => NodeKind::Emphasis,
                    Tag::Strong => NodeKind::Strong,
                    Tag::Strikethrough => NodeKind::Strikethrough,
                    Tag::Link { link_type: LinkType::WikiLink { has_pothole }, dest_url, .. } => {
                        NodeKind::WikiLink { has_pothole, dest_len: dest_url.len() as u32 }
                    }
                    Tag::Link { link_type, id, .. } => {
                        NodeKind::Link { reference: reference_id(link_type, id.as_ref()) }
                    }
                    Tag::Image { link_type: LinkType::WikiLink { has_pothole }, dest_url, .. } => {
                        NodeKind::WikiImage { has_pothole, dest_len: dest_url.len() as u32 }
                    }
                    Tag::Image { link_type, id, .. } => {
                        NodeKind::Image { reference: reference_id(link_type, id.as_ref()) }
                    }
                    Tag::Table(_) => NodeKind::Table,
                    Tag::HtmlBlock => NodeKind::HtmlBlock,
                    _ => NodeKind::Other,
                };
                // A frame's quote_depth counts *enclosing* quotes, excluding
                // the node itself when it is a quote.
                let enclosing_quotes = quote_depth;
                if matches!(node, NodeKind::BlockQuote) {
                    quote_depth += 1;
                }
                // Captured BEFORE the increment: the table frame itself is not
                // "in a table", so its own whole-table marker is never suppressed.
                let enclosing_table = table_depth > 0;
                if matches!(node, NodeKind::Table) {
                    table_depth += 1;
                }
                stack.push(Frame {
                    node,
                    range: r,
                    first_child: u32::MAX,
                    last_child_end: 0,
                    quote_depth: enclosing_quotes,
                    in_table: enclosing_table,
                    task: None,
                });
            }
            Event::End(_) => {
                let frame = stack.pop().expect("balanced events");
                if matches!(frame.node, NodeKind::BlockQuote) {
                    quote_depth -= 1;
                }
                if matches!(frame.node, NodeKind::Table) {
                    table_depth -= 1;
                }
                finish_node(text, frame, r, &def_urls, extensions, out);
            }
            Event::Text(_) => note_child(&mut stack, r),
            Event::Code(_) => {
                note_child(&mut stack, r);
                code_span(text, r, out, table_depth > 0);
            }
            Event::InlineHtml(_) | Event::Html(_) => {
                note_child(&mut stack, r);
                out.paints.push(Paint {
                    range: r,
                    kind: Kind::HTML_RAW,
                });
            }
            Event::Rule => {
                note_child(&mut stack, r);
                if stack.is_empty() {
                    out.blocks.push(r);
                }
                let hr = trim_trailing_newline(text, r);
                out.paints.push(Paint {
                    range: hr,
                    kind: Kind::THEMATIC_BREAK,
                });
                // Co-located Block-scoped marker: the rule line conceals when
                // the caret is off it (host draws a divider in its place) and
                // reveals the raw `---` on it — same line policy as `> `.
                // Quoted rules stay raw (no marker), the same v1 rule as
                // quoted tables: the quote paragraph style owns the row, so
                // there is no pinned height for a host to draw a divider into.
                if quote_depth == 0 {
                    push_marker(out, hr, Kind::MARKER, hr, MarkerScope::Block);
                }
            }
            Event::TaskListMarker(checked) => {
                note_child(&mut stack, r);
                if let Some(item) = stack.iter_mut().rev().find(|f| matches!(f.node, NodeKind::Item)) {
                    item.task = Some((r, checked));
                }
            }
            Event::SoftBreak | Event::HardBreak => note_child(&mut stack, r),
            _ => note_child(&mut stack, r),
        }
    }
    debug_assert!(stack.is_empty(), "unbalanced pulldown events");

    // Conceal each reference definition line (§2.3 b1): a Block-scoped MARKER
    // over the def span hides it off-caret and reveals raw source on-caret,
    // mirroring the thematic-break precedent. Blockquoted defs stay raw (same
    // v1 rule as quoted rules/tables) — the quote's per-line `> ` markers would
    // otherwise overlap. push_marker fails closed on a reversed/empty span.
    //
    // Unlike `Event::Rule`, def spans are deliberately NOT pushed into
    // `out.blocks`: a def can sit inside a list item's block, so registering it
    // would break the sorted+disjoint invariant `reveal_region` relies on. The
    // Block-scoped marker still reveals correctly under `RevealMode::Line`/
    // `Element` (the only modes shipping apps select — iOS Line, macOS None/
    // Element) via `line_region`; under the unused `RevealMode::Block` a def
    // reveals only when the caret is strictly inside its span.
    //
    // Known v1 limitation: pulldown's RefDefs keeps ONE span per label (the
    // first, per CommonMark's first-definition-wins), so a DUPLICATE-label
    // definition line has no span here and renders as raw body text while the
    // first conceals. Rare (duplicate defs are usually authoring mistakes) and
    // honest — the raw line is visibly editable.
    for span in &def_spans {
        if def_in_blockquote(text, *span) {
            continue;
        }
        push_marker(out, *span, Kind::MARKER, *span, MarkerScope::Block);
    }

    // Element ids are assigned in document order (stable within a parse
    // generation, deterministic).
    out.elements
        .sort_by_key(|e| (e.range.start, e.range.end, element_sort_key(e)));
    for (i, e) in out.elements.iter_mut().enumerate() {
        e.id = i as u32;
    }
    out.elements_max_end.clear();
    let mut max_end = 0u32;
    for e in &out.elements {
        max_end = max_end.max(e.range.end);
        out.elements_max_end.push(max_end);
    }
    out.task_boxes.sort_by_key(|t| (t.item.start, t.item.end));
    out.verbatim_blocks.sort_by_key(|r| r.start);
}

fn element_sort_key(e: &Element) -> u32 {
    match e.kind {
        ElementKind::Task { .. } => 0,
        ElementKind::Link { .. } => 1,
        ElementKind::Image { .. } => 2,
        ElementKind::Fence { .. } => 3,
        ElementKind::WikiLink { .. } => 4,
        ElementKind::Heading { .. } => 5,
    }
}

fn note_child(stack: &mut [Frame], r: ByteRange) {
    if let Some(top) = stack.last_mut() {
        if top.first_child == u32::MAX {
            top.first_child = r.start;
        }
        top.last_child_end = top.last_child_end.max(r.end);
    }
}

fn trim_trailing_newline(text: &str, r: ByteRange) -> ByteRange {
    if r.end > r.start && text.as_bytes()[r.end as usize - 1] == b'\n' {
        ByteRange::new(r.start, r.end - 1)
    } else {
        r
    }
}

/// Paints `kind` over `range` minus the (sorted, disjoint) `holes`.
fn paint_minus(range: ByteRange, holes: &[ByteRange], kind: Kind, out: &mut ParseOut) {
    let mut pos = range.start;
    for h in holes {
        if h.start > pos {
            out.paints.push(Paint {
                range: ByteRange::new(pos, h.start.min(range.end)),
                kind,
            });
        }
        pos = pos.max(h.end);
    }
    if pos < range.end {
        out.paints.push(Paint {
            range: ByteRange::new(pos, range.end),
            kind,
        });
    }
}

fn push_marker(out: &mut ParseOut, range: ByteRange, kind: Kind, owner: ByteRange, scope: MarkerScope) {
    // Fail closed: downstream (`flatten`, `current_owner`) guards
    // well-formedness with debug_asserts only, so a reversed range would
    // silently corrupt every segment after it in release. Drop it instead —
    // a missing marker degrades one node's conceal, not the document.
    if range.start < range.end {
        out.markers.push(MarkerPaint { range, kind, owner, scope });
    }
}

/// Skips `depth` blockquote prefixes (`up to 3 spaces, '>', optional space`)
/// starting at `pos`, bounded by `line_end`. Returns the position after them,
/// or None if the line doesn't carry that many prefixes (lazy continuation).
fn skip_quote_prefixes(text: &str, mut pos: u32, line_end: u32, depth: u32) -> Option<u32> {
    let bytes = text.as_bytes();
    for _ in 0..depth {
        let (_, after) = one_quote_prefix(bytes, pos, line_end)?;
        pos = after;
    }
    Some(pos)
}

/// Consumes ONE blockquote prefix (up to 3 spaces, `>`, optional space) at
/// `pos`, bounded by `line_end`. Returns `(marker_start, after)` where
/// `marker_start` is the `>` byte — the single definition of the prefix
/// shape shared by parse and behaviors.
pub(crate) fn one_quote_prefix(bytes: &[u8], pos: u32, line_end: u32) -> Option<(u32, u32)> {
    let mut p = pos;
    let mut spaces = 0;
    while p < line_end && bytes[p as usize] == b' ' && spaces < 3 {
        p += 1;
        spaces += 1;
    }
    if p < line_end && bytes[p as usize] == b'>' {
        let marker = p;
        p += 1;
        if p < line_end && bytes[p as usize] == b' ' {
            p += 1;
        }
        Some((marker, p))
    } else {
        None
    }
}

fn finish_node(
    text: &str,
    frame: Frame,
    end_range: ByteRange,
    def_urls: &[(UniCase<String>, ByteRange)],
    extensions: Extensions,
    out: &mut ParseOut,
) {
    // Start and End event ranges are identical for the same node (probed).
    debug_assert_eq!(frame.range, end_range);
    let r = frame.range;
    match frame.node {
        NodeKind::Heading(level) => heading(text, &frame, out, level),
        NodeKind::BlockQuote => blockquote(text, &frame, out),
        NodeKind::FencedCode => fenced_code(text, &frame, extensions, out),
        NodeKind::IndentedCode => {
            out.paints.push(Paint {
                range: r,
                kind: Kind::CODE_BLOCK,
            });
            out.verbatim_blocks.push(r);
        }
        NodeKind::Item => list_item(text, &frame, out),
        NodeKind::Emphasis => inline_span(&frame, Kind::EMPHASIS, out),
        NodeKind::Strong => inline_span(&frame, Kind::STRONG, out),
        NodeKind::Strikethrough => inline_span(&frame, Kind::STRIKETHROUGH, out),
        NodeKind::Link { .. } => link_or_image(text, &frame, def_urls, out, false),
        NodeKind::WikiLink { has_pothole, dest_len } => {
            wikilink(text, &frame, out, has_pothole, dest_len)
        }
        NodeKind::Image { .. } => link_or_image(text, &frame, def_urls, out, true),
        NodeKind::WikiImage { has_pothole, dest_len } => {
            wiki_image(text, &frame, out, has_pothole, dest_len)
        }
        NodeKind::Table => {
            let table = trim_trailing_newline(text, r);
            out.paints.push(Paint {
                range: r,
                kind: Kind::TABLE,
            });
            // Co-located Block-scoped marker over the table body (newline
            // excluded, HR precedent): concealed off-caret so hosts can draw a
            // grid view in its place, revealed to raw pipe-text whenever the
            // caret's line touches ANY table line (owner-intersection policy).
            // NEVER inside a blockquote: the quote's per-line `> ` markers live
            // inside the table's range there, and markers must stay disjoint —
            // a quoted table simply renders raw (documented v1 limitation).
            if frame.quote_depth == 0 {
                push_marker(out, table, Kind::MARKER, table, MarkerScope::Block);
            }
        }
        NodeKind::HtmlBlock => out.verbatim_blocks.push(r),
        NodeKind::Other => {}
    }
}

/// Inside a table, inline markers/elements are suppressed (the whole-table
/// Block marker owns conceal): paint just the visible body, emit nothing
/// else. Returns true when the frame was handled — callers return
/// immediately. One helper so the rule can't drift across the three inline
/// constructs.
fn paint_table_body(frame: &Frame, kind: Kind, out: &mut ParseOut) -> bool {
    if !frame.in_table {
        return false;
    }
    if let Some(c) = frame.child_span() {
        out.paints.push(Paint { range: c, kind });
    }
    true
}

/// Emphasis / strong / strikethrough: delimiters are the gaps between the
/// node range and its direct children's span.
fn inline_span(frame: &Frame, kind: Kind, out: &mut ParseOut) {
    let r = frame.range;
    if paint_table_body(frame, kind, out) {
        return;
    }
    match frame.child_span() {
        Some(c) => {
            push_marker(out, ByteRange::new(r.start, c.start), Kind::MARKER, r, MarkerScope::Inline);
            push_marker(out, ByteRange::new(c.end, r.end), Kind::MARKER, r, MarkerScope::Inline);
            out.paints.push(Paint { range: c, kind });
        }
        None => push_marker(out, r, Kind::MARKER, r, MarkerScope::Inline),
    }
}

fn heading(text: &str, frame: &Frame, out: &mut ParseOut, level: u8) {
    let r = frame.range;
    let bytes = text.as_bytes();
    let mut own: Vec<ByteRange> = Vec::new();

    // ATX iff the first line is `#{1..6}` followed by space/tab/EOL.
    let content_line_end = {
        let mut e = r.start;
        while e < r.end && bytes[e as usize] != b'\n' {
            e += 1;
        }
        e
    };
    let mut hash_end = r.start;
    while hash_end < content_line_end && bytes[hash_end as usize] == b'#' {
        hash_end += 1;
    }
    let hashes = hash_end - r.start;
    let is_atx = (1..=6).contains(&hashes)
        && (hash_end == content_line_end
            || bytes[hash_end as usize] == b' '
            || bytes[hash_end as usize] == b'\t'
            || bytes[hash_end as usize] == b'\r');

    if is_atx {
        // Opening marker: `#`-run plus one following space/tab.
        let mut m_end = hash_end;
        if m_end < content_line_end && (bytes[m_end as usize] == b' ' || bytes[m_end as usize] == b'\t') {
            m_end += 1;
        }
        own.push(ByteRange::new(r.start, m_end));

        // Optional closing sequence: spaces + `#`-run (+ trailing spaces) at EOL.
        // A CRLF line ends in `\r` here (content_line_end stops at `\n`) —
        // step past it or the closing run is never recognized.
        let mut e = content_line_end;
        if e > m_end && bytes[e as usize - 1] == b'\r' {
            e -= 1;
        }
        while e > m_end && (bytes[e as usize - 1] == b' ' || bytes[e as usize - 1] == b'\t') {
            e -= 1;
        }
        let run_end = e;
        while e > m_end && bytes[e as usize - 1] == b'#' {
            e -= 1;
        }
        if e < run_end {
            // A closing run only counts when preceded by whitespace.
            let mut ws = e;
            while ws > m_end && (bytes[ws as usize - 1] == b' ' || bytes[ws as usize - 1] == b'\t') {
                ws -= 1;
            }
            if ws < e && ws > m_end {
                own.push(ByteRange::new(ws, content_line_end));
            }
        }
    } else {
        // Setext: the underline is the last line of the node range.
        let trimmed = trim_trailing_newline(text, r);
        let mut ls = trimmed.end;
        while ls > r.start && bytes[ls as usize - 1] != b'\n' {
            ls -= 1;
        }
        if let Some(p) = skip_quote_prefixes(text, ls, trimmed.end, frame.quote_depth) {
            let mut run = p;
            while run < trimmed.end && bytes[run as usize] == b' ' && run - p < 3 {
                run += 1;
            }
            if run < trimmed.end && (bytes[run as usize] == b'=' || bytes[run as usize] == b'-') {
                own.push(ByteRange::new(run, trimmed.end));
            }
        }
    }

    own.sort_by_key(|m| m.start);
    for m in &own {
        push_marker(out, *m, Kind::MARKER, r, MarkerScope::Block);
    }
    paint_minus(r, &own, Kind::heading(level), out);

    let text_range = if is_atx {
        let mut start = own.first().map_or(r.start, |m| m.end);
        let mut end = own.get(1).map_or(content_line_end, |m| m.start);
        while start < end && matches!(bytes[start as usize], b' ' | b'\t') {
            start += 1;
        }
        while end > start && matches!(bytes[end as usize - 1], b' ' | b'\t' | b'\r') {
            end -= 1;
        }
        ByteRange::new(start, end)
    } else {
        let mut end = own.first().map_or(r.end, |m| m.start);
        // The underline line's own container prefix (e.g. `> `) sits between
        // the last content byte and the marker run — it is not title text.
        let mut nl = end;
        while nl > r.start && bytes[nl as usize - 1] != b'\n' {
            nl -= 1;
        }
        if nl > r.start
            && bytes[nl as usize..end as usize]
                .iter()
                .all(|&b| matches!(b, b'>' | b' ' | b'\t'))
        {
            end = nl;
        }
        let mut start = r.start;
        while start < end && matches!(bytes[start as usize], b' ' | b'\t') {
            start += 1;
        }
        while end > start && matches!(bytes[end as usize - 1], b'\n' | b'\r' | b' ' | b'\t') {
            end -= 1;
        }
        ByteRange::new(start, end)
    };
    out.elements.push(Element {
        id: 0, // reassigned in document order
        range: r,
        kind: ElementKind::Heading { level, text: text_range },
    });
}

fn blockquote(text: &str, frame: &Frame, out: &mut ParseOut) {
    let r = frame.range;
    let bytes = text.as_bytes();
    let mut own: Vec<ByteRange> = Vec::new();

    // Walk each physical line intersecting the node range; the node's own `>`
    // is the (depth+1)-th prefix on the line (enclosing quotes own the others).
    let mut line_start = {
        let mut p = r.start;
        while p > 0 && bytes[p as usize - 1] != b'\n' {
            p -= 1;
        }
        p
    };
    while line_start < r.end {
        let mut line_end = line_start;
        while line_end < (text.len() as u32) && bytes[line_end as usize] != b'\n' {
            line_end += 1;
        }
        let scan_from = if line_start < r.start { r.start } else { line_start };
        let depth = if line_start < r.start {
            // First line: enclosing quote markers sit before r.start already.
            0
        } else {
            frame.quote_depth
        };
        if let Some(p) = skip_quote_prefixes(text, scan_from, line_end, depth)
            && let Some((m_start, after)) = one_quote_prefix(bytes, p, line_end)
        {
            own.push(ByteRange::new(m_start, after.min(r.end)));
        }
        line_start = line_end + 1;
    }

    for m in &own {
        push_marker(out, *m, Kind::MARKER, r, MarkerScope::Block);
    }
    // Full-range paint (markers included): the `> ` runs compose
    // BLOCKQUOTE|MARKER, so a theme's paragraph style can key off the
    // quote bit at the paragraph's first character (the F7/TASK pattern),
    // and hosts can coalesce quote blocks from one uniform kind bit.
    out.paints.push(Paint {
        range: r,
        kind: Kind::BLOCKQUOTE,
    });
}

fn fenced_code(text: &str, frame: &Frame, extensions: Extensions, out: &mut ParseOut) {
    let r = frame.range;
    let bytes = text.as_bytes();
    let mut own: Vec<ByteRange> = Vec::new();
    let mut info_range = ByteRange::new(r.start, r.start);

    // Opening line: optional indent, fence run, optional info string.
    let line1_end = {
        let mut e = r.start;
        while e < r.end && bytes[e as usize] != b'\n' {
            e += 1;
        }
        e
    };
    let mut p = r.start;
    while p < line1_end && bytes[p as usize] == b' ' {
        p += 1;
    }
    let fence_char = if p < line1_end { bytes[p as usize] } else { 0 };
    let mut open_run_end = p;
    if fence_char == b'`' || fence_char == b'~' {
        while open_run_end < line1_end && bytes[open_run_end as usize] == fence_char {
            open_run_end += 1;
        }
        own.push(ByteRange::new(p, open_run_end));
        // Info string: trimmed remainder of the opening line.
        let mut is = open_run_end;
        while is < line1_end && bytes[is as usize] == b' ' {
            is += 1;
        }
        let mut ie = line1_end;
        while ie > is && bytes[ie as usize - 1] == b' ' {
            ie -= 1;
        }
        info_range = ByteRange::new(is, ie);
        if !info_range.is_empty() {
            own.push(info_range);
        } else {
            info_range = ByteRange::new(open_run_end, open_run_end);
        }
    }

    // Closing line: last line of the range (unless it IS the opening line),
    // possibly behind blockquote prefixes.
    let trimmed = trim_trailing_newline(text, r);
    let mut ls = trimmed.end;
    while ls > r.start && bytes[ls as usize - 1] != b'\n' {
        ls -= 1;
    }
    if ls > line1_end
        && (fence_char == b'`' || fence_char == b'~')
        && let Some(q0) = skip_quote_prefixes(text, ls, trimmed.end, frame.quote_depth)
    {
        let mut q = q0;
        let mut spaces = 0;
        while q < trimmed.end && bytes[q as usize] == b' ' && spaces < 3 {
            q += 1;
            spaces += 1;
        }
        let run_start = q;
        while q < trimmed.end && bytes[q as usize] == fence_char {
            q += 1;
        }
        let run_len = q - run_start;
        let mut rest = q;
        while rest < trimmed.end && (bytes[rest as usize] == b' ' || bytes[rest as usize] == b'\t') {
            rest += 1;
        }
        if run_len >= 3 && run_len >= (open_run_end - p) && rest == trimmed.end {
            own.push(ByteRange::new(run_start, q));
        }
    }

    // Structural code (`Extensions::STRUCTURAL_CODE`, host opt-in): ONE
    // Block-scoped marker over the whole block replaces the fence/info
    // markers — the table model. Concealed off-caret so the host draws a
    // horizontally scrolling code view in the reserved space; revealed to the
    // raw fences+body whenever the caret's line touches the block. Never
    // inside a blockquote (the quote's per-line `> ` markers live inside the
    // block's range and markers must stay disjoint — the quoted-table rule).
    if extensions.contains(Extensions::STRUCTURAL_CODE) && frame.quote_depth == 0 {
        let code = trim_trailing_newline(text, r);
        push_marker(out, code, Kind::MARKER, code, MarkerScope::Block);
        out.paints.push(Paint {
            range: r,
            kind: Kind::CODE_BLOCK,
        });
    } else {
        own.sort_by_key(|m| m.start);
        for m in &own {
            let kind = if *m == info_range && !info_range.is_empty() {
                Kind::FENCE_INFO
            } else {
                Kind::MARKER
            };
            push_marker(out, *m, kind, r, MarkerScope::Block);
        }
        paint_minus(r, &own, Kind::CODE_BLOCK, out);
    }

    out.verbatim_blocks.push(r);
    out.elements.push(Element {
        id: 0,
        range: r,
        kind: ElementKind::Fence { info: info_range },
    });
}

fn list_item(text: &str, frame: &Frame, out: &mut ParseOut) {
    let r = frame.range;
    let bytes = text.as_bytes();
    let p = r.start;

    // Bullet or ordinal token at the item start (pulldown item ranges begin
    // exactly at the marker; probed).
    let token: Option<(ByteRange, Kind)> = if p < r.end && matches!(bytes[p as usize], b'-' | b'*' | b'+') {
        Some((ByteRange::new(p, p + 1), Kind::LIST_BULLET))
    } else {
        let mut d = p;
        while d < r.end && bytes[d as usize].is_ascii_digit() && d - p < 9 {
            d += 1;
        }
        if d > p && d < r.end && matches!(bytes[d as usize], b'.' | b')') {
            Some((ByteRange::new(p, d + 1), Kind::LIST_ORDINAL))
        } else {
            None
        }
    };
    let Some((token_range, token_kind)) = token else {
        return;
    };

    if let Some((boxx, checked)) = frame.task {
        // Task item: the whole `- [ ]` prefix (bullet through closing bracket,
        // plus one trailing space) is TASK_MARKER, conceal class Marker.
        // LIST_BULLET's never-conceal rule applies to non-task
        // items only.
        let mut m_end = boxx.end;
        if (m_end as usize) < text.len() && bytes[m_end as usize] == b' ' {
            m_end += 1;
        }
        // Reveal owner is the marker's own span, not the item `r`. A list item
        // swallows lazy-continuation lines (CommonMark 5.2 — a following
        // unindented paragraph line joins the item), so `r` reaches lines the
        // marker doesn't sit on, and a caret parked on one of them would hold
        // the `- [ ]` revealed. `Block` scope means "conceal per the line the
        // marker sits on"; the marker never spans lines, so its own range is a
        // faithful stand-in for that line against the whole-line reveal region.
        // Fenced blocks deliberately differ (owner = the whole block) so their
        // fences and info string stay legible while the caret is inside.
        let marker = ByteRange::new(r.start, m_end);
        push_marker(out, marker, Kind::TASK_MARKER, marker, MarkerScope::Block);
        out.task_boxes.push(TaskBox {
            item: r,
            boxx,
            checked,
        });
        out.elements.push(Element {
            id: 0,
            range: r,
            kind: ElementKind::Task { checked },
        });
    } else {
        out.paints.push(Paint {
            range: token_range,
            kind: token_kind,
        });
    }
}

fn link_or_image(
    text: &str,
    frame: &Frame,
    def_urls: &[(UniCase<String>, ByteRange)],
    out: &mut ParseOut,
    image: bool,
) {
    let r = frame.range;
    let kind = if image { Kind::IMAGE } else { Kind::LINK };
    if paint_table_body(frame, kind, out) {
        return;
    }
    // No body-less special case here (unlike `wikilink`): links close with a
    // single `]`, so a closer-offset filter would swallow the label of
    // 1-character reference links like `[1]` (critic-caught regression). The
    // `child_span` clamp + fail-closed `push_marker` already guarantee
    // well-formed output for pulldown's corrupted re-walk; a re-walked
    // sibling merely renders its `]]` visibly, which is transient cosmetics.
    let (lead, trail) = match frame.child_span() {
        Some(c) => (
            ByteRange::new(r.start, c.start),
            ByteRange::new(c.end, r.end),
        ),
        None => {
            let opener = r.start + if image { 2 } else { 1 };
            (
                ByteRange::new(r.start, opener.min(r.end)),
                ByteRange::new(opener.min(r.end), r.end),
            )
        }
    };
    push_marker(out, lead, Kind::MARKER, r, MarkerScope::Inline);
    push_marker(out, trail, Kind::MARKER, r, MarkerScope::Inline);
    if let Some(c) = frame.child_span() {
        out.paints.push(Paint { range: c, kind });
    }

    // Reference/collapsed/shortcut links carry their label on the node; their
    // dest lives in a definition, so binary-search the label-sorted def index
    // (§2.1). `UniCase` folding matches pulldown's own map exactly, so any link
    // pulldown resolved is guaranteed to find its def here — including
    // non-ASCII labels differing only in case. Inline/autolink dests still come
    // from scan_dest.
    let reference = match &frame.node {
        NodeKind::Link { reference } | NodeKind::Image { reference } => reference.as_deref(),
        _ => None,
    };
    let dest = match reference {
        Some(label) => {
            let needle = UniCase::new(label.to_string());
            def_urls
                .binary_search_by(|(l, _)| l.cmp(&needle))
                .map_or(ByteRange::new(r.end, r.end), |i| def_urls[i].1)
        }
        None => scan_dest(text, r, trail),
    };
    out.elements.push(Element {
        id: 0,
        range: r,
        kind: if image {
            ElementKind::Image { src: dest, wiki: false }
        } else {
            ElementKind::Link { dest }
        },
    });
}

/// Locates the destination byte range inside the source text.
/// Inline `[t](dest "title")` → inside the parens; autolink `<dest>` → inner
/// range; reference/collapsed/shortcut links → empty range at the node end
/// (the definition lives elsewhere; v0 does not track definition offsets).
fn scan_dest(text: &str, node: ByteRange, trail: ByteRange) -> ByteRange {
    let bytes = text.as_bytes();
    let t = &text[trail.start as usize..trail.end as usize];
    if let Some(rel) = t.find("](") {
        let mut p = trail.start + rel as u32 + 2;
        while p < trail.end && bytes[p as usize] == b' ' {
            p += 1;
        }
        if p < trail.end && bytes[p as usize] == b'<' {
            let ds = p + 1;
            let mut de = ds;
            while de < trail.end && bytes[de as usize] != b'>' {
                de += 1;
            }
            return ByteRange::new(ds, de);
        }
        let ds = p;
        let mut depth = 0i32;
        let mut de = ds;
        while de < trail.end {
            match bytes[de as usize] {
                b'\\' => de += 1,
                b'(' => depth += 1,
                b')' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                b' ' | b'\t' | b'\n' => break,
                _ => {}
            }
            de += 1;
        }
        return ByteRange::new(ds, de.min(trail.end));
    }
    // Autolink: `<dest>` — the whole node is the destination.
    if node.len() >= 2 && bytes[node.start as usize] == b'<' && bytes[node.end as usize - 1] == b'>' {
        return ByteRange::new(node.start + 1, node.end - 1);
    }
    ByteRange::new(node.end, node.end)
}

/// Maps a link/image `LinkType` to its reference label, if it is a
/// reference/collapsed/shortcut form (whose destination lives in a definition,
/// invisible to `scan_dest` at the link site). Inline/autolink/email carry the
/// dest inline and return `None`. The `*Unknown` variants only occur with a
/// broken-link callback (we install none) — matched defensively.
fn reference_id(link_type: LinkType, id: &str) -> Option<String> {
    match link_type {
        LinkType::Reference
        | LinkType::ReferenceUnknown
        | LinkType::Collapsed
        | LinkType::CollapsedUnknown
        | LinkType::Shortcut
        | LinkType::ShortcutUnknown => Some(id.to_string()),
        _ => None,
    }
}

/// Byte range of the URL inside a definition `span`. Mirrors `scan_dest`'s
/// angle-bracket / bare-token rules: skip the label `[...]` (honoring `\]`),
/// the `:`, and leading spaces/tabs; `<url>` yields the inner bytes, otherwise
/// the URL runs to the next whitespace or `span.end`. Title text and a
/// title-continuation line (both after whitespace) are excluded.
fn scan_def_url(text: &str, span: ByteRange) -> ByteRange {
    let bytes = text.as_bytes();
    let end = span.end;
    let mut p = span.start;
    if p < end && bytes[p as usize] == b'[' {
        p += 1;
        while p < end && bytes[p as usize] != b']' {
            if bytes[p as usize] == b'\\' {
                p += 1;
            }
            p += 1;
        }
        if p < end {
            p += 1; // past the closing `]`
        }
    }
    if p < end && bytes[p as usize] == b':' {
        p += 1;
    }
    // Whitespace between `:` and the URL, INCLUDING a line ending — CommonMark
    // allows the destination on the line after the label (`[ref]:\nurl`). The
    // span boundary already excludes any blank-line gap, so crossing newlines
    // here can never overshoot past the destination. A QUOTED def's
    // continuation line re-carries its `> ` prefix(es) inside the span — skip
    // them too, or the "URL" would resolve to the `>` byte.
    while p < end {
        match bytes[p as usize] {
            b' ' | b'\t' | b'\r' => p += 1,
            b'\n' => {
                p += 1;
                while let Some((_, after)) = one_quote_prefix(bytes, p, end) {
                    p = after;
                }
            }
            _ => break,
        }
    }
    if p < end && bytes[p as usize] == b'<' {
        let ds = p + 1;
        let mut de = ds;
        while de < end && bytes[de as usize] != b'>' {
            de += 1;
        }
        return ByteRange::new(ds, de);
    }
    let ds = p;
    let mut de = ds;
    while de < end && !matches!(bytes[de as usize], b' ' | b'\t' | b'\n' | b'\r') {
        de += 1;
    }
    ByteRange::new(ds, de)
}

/// True when a definition line carries a blockquote `>` prefix (scan the line
/// head before `span.start`). Such defs render raw, matching the quoted
/// thematic-break / table v1 rule.
fn def_in_blockquote(text: &str, span: ByteRange) -> bool {
    let bytes = text.as_bytes();
    let mut ls = span.start;
    while ls > 0 && bytes[ls as usize - 1] != b'\n' {
        ls -= 1;
    }
    bytes[ls as usize..span.start as usize].contains(&b'>')
}

/// Wikilinks (`[[target]]` / `[[target|alias]]`, pulldown `ENABLE_WIKILINKS`).
/// pulldown emits these through `Tag::Link` with `LinkType::WikiLink`, whose
/// single `Text` child is the *visible* body — the alias when piped, the
/// target otherwise — spanning exactly the bytes that stay legible. The gap
/// technique therefore paints the concealed runs for free: `[[` (plus
/// `target|` when piped) becomes the lead marker, `]]` the trail marker, and
/// the child span is the visible `LINK` body. Only the element's `target`
/// needs deriving: pulldown's `dest_url` is an owned `CowStr` with no source
/// offsets.
fn wikilink(text: &str, frame: &Frame, out: &mut ParseOut, has_pothole: bool, dest_len: u32) {
    let r = frame.range;
    if paint_table_body(frame, Kind::LINK, out) {
        return;
    }
    match frame.child_span() {
        // An empty piped alias (`[[a|]]`) has no legible body — pulldown
        // re-classifies the closing `]]` as two Text children. Anything
        // starting inside the trailing `]]` is that junk: conceal the whole
        // node. The element below still carries the target, so navigation
        // works even in this mid-typing state.
        Some(c) if c.start >= r.end.saturating_sub(2) => {
            push_marker(out, r, Kind::MARKER, r, MarkerScope::Inline);
        }
        Some(c) => {
            push_marker(out, ByteRange::new(r.start, c.start), Kind::MARKER, r, MarkerScope::Inline);
            push_marker(out, ByteRange::new(c.end, r.end), Kind::MARKER, r, MarkerScope::Inline);
            out.paints.push(Paint { range: c, kind: Kind::LINK });
        }
        // Body-less (empty wikiname never parses; kept as a defensive arm).
        None => push_marker(out, r, Kind::MARKER, r, MarkerScope::Inline),
    }

    let target = wikilink_target(text, frame, has_pothole, dest_len, 2);
    out.elements.push(Element {
        id: 0,
        range: r,
        kind: ElementKind::WikiLink { target },
    });
}

/// Wikilink image embeds (`![[file.png]]` / `![[file.png|alias]]`, Obsidian
/// attachments). Structurally a wikilink whose element is an image: pulldown
/// emits it via `Tag::Image` with `LinkType::WikiLink`, so `scan_dest` (which
/// only reads `[t](dest)` / `<autolink>`) would return an empty src and the
/// host could never resolve the file. Marker/conceal mirror `wikilink` (`![[`
/// plus a piped `target|` lead, `]]` trail, visible body = the child span); the
/// body paints `IMAGE` so the alt line renders like a markdown embed, and the
/// element carries the bare target as `src` for the host to resolve against
/// the vault.
fn wiki_image(text: &str, frame: &Frame, out: &mut ParseOut, has_pothole: bool, dest_len: u32) {
    let r = frame.range;
    if paint_table_body(frame, Kind::IMAGE, out) {
        return;
    }
    match frame.child_span() {
        // Empty piped alias (`![[a|]]`): pulldown re-classifies the closing
        // `]]` as Text children — conceal the whole node, the element below
        // still carries the target so resolution works mid-typing.
        Some(c) if c.start >= r.end.saturating_sub(2) => {
            push_marker(out, r, Kind::MARKER, r, MarkerScope::Inline);
        }
        Some(c) => {
            push_marker(out, ByteRange::new(r.start, c.start), Kind::MARKER, r, MarkerScope::Inline);
            push_marker(out, ByteRange::new(c.end, r.end), Kind::MARKER, r, MarkerScope::Inline);
            out.paints.push(Paint { range: c, kind: Kind::IMAGE });
        }
        None => push_marker(out, r, Kind::MARKER, r, MarkerScope::Inline),
    }

    let src = wikilink_target(text, frame, has_pothole, dest_len, 3);
    out.elements.push(Element {
        id: 0,
        range: r,
        kind: ElementKind::Image { src, wiki: true },
    });
}

/// Locates the wikilink target range. pulldown's `dest_url` is an owned
/// `CowStr` with no source offsets, but it is a verbatim borrow of the target
/// run, so `dest_len` pins the range once one end is anchored — and the child
/// span anchors it. Plain form: pulldown builds the body node *from* the
/// target run, so the child span is the target. Piped form: pulldown restarts
/// the body one byte past the `|` it split on (the first *raw* one, with no
/// unescaping — so `[[a\|b|c]]` targets `a\`), so the target is the `dest_len`
/// bytes ending just before the visible alias.
///
/// Anchoring is not the same as counting `opener` bytes from the node start:
/// pulldown resumes the run after the *last* doubled `]]` the body swallows,
/// so `![[[a]]b]]` targets `b`, not the `[a]]b` that starts right after `![[`.
///
/// `opener` — the marker byte width, `2` for `[[…]]` and `3` for `![[…]]` —
/// only feeds the body-less fallback, which has no child span to anchor to.
fn wikilink_target(
    text: &str,
    frame: &Frame,
    has_pothole: bool,
    dest_len: u32,
    opener: u32,
) -> ByteRange {
    match frame.child_span() {
        Some(c) if has_pothole => {
            let pipe = c.start.saturating_sub(1);
            ByteRange::new(pipe.saturating_sub(dest_len), pipe)
        }
        Some(c) => c,
        // Body-less (empty wikiname never parses; kept as a defensive arm).
        // The node always has the shape `[[…]]` with a non-empty name here, so
        // `node.len() >= 5` and the `+opener / -2` never cross or underflow.
        None => {
            let node = frame.range;
            let start = node.start + opener;
            let inner_end = node.end - 2;
            if has_pothole {
                let bytes = text.as_bytes();
                let mut p = start;
                while p < inner_end && bytes[p as usize] != b'|' {
                    p += 1;
                }
                ByteRange::new(start, p)
            } else {
                ByteRange::new(start, inner_end)
            }
        }
    }
}

fn code_span(text: &str, r: ByteRange, out: &mut ParseOut, in_table: bool) {
    let bytes = text.as_bytes();
    let mut open = r.start;
    while open < r.end && bytes[open as usize] == b'`' {
        open += 1;
    }
    let mut close = r.end;
    while close > open && bytes[close as usize - 1] == b'`' {
        close -= 1;
    }
    if !in_table {
        push_marker(out, ByteRange::new(r.start, open), Kind::MARKER, r, MarkerScope::Inline);
        push_marker(out, ByteRange::new(close, r.end), Kind::MARKER, r, MarkerScope::Inline);
    }
    if open < close {
        out.paints.push(Paint {
            range: ByteRange::new(open, close),
            kind: Kind::CODE_SPAN,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `push_marker` site declares its own scope; it is not
    /// recoverable from `kind` afterward (heading and strong
    /// markers both paint the bare `MARKER` bit). One document per
    /// construct pins the scope at each site exhaustively.
    #[test]
    fn scope_assigned_at_every_push_marker_site() {
        let cases: &[(&str, MarkerScope)] = &[
            ("# heading\n", MarkerScope::Block),
            ("> quote\n", MarkerScope::Block),
            ("```\nx\n```\n", MarkerScope::Block),
            ("```rust\nx\n```\n", MarkerScope::Block),
            ("- [ ] t\n", MarkerScope::Block),
            ("**b**\n", MarkerScope::Inline),
            ("*i*\n", MarkerScope::Inline),
            ("~~s~~\n", MarkerScope::Inline),
            ("`c`\n", MarkerScope::Inline),
            ("[l](u)\n", MarkerScope::Inline),
            ("![i](s)\n", MarkerScope::Inline),
        ];
        for (text, expected) in cases {
            let mut po = ParseOut::default();
            parse(text, Extensions::all(), &mut po);
            assert!(!po.markers.is_empty(), "no markers parsed for {text:?}");
            for m in &po.markers {
                assert_eq!(m.scope, *expected, "wrong scope for marker in {text:?}: {m:?}");
            }
        }
    }
}
