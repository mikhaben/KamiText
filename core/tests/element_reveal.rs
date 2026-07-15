//! `RevealMode::Element` engine-level tests: the predicate edge matrix
//! (boundaries, adjacent elements, EOF, selection endpoints), block-vs-inline
//! scope, and the patch-completeness proptest guarding against a within-line
//! caret move into an element that must still produce a patch, even though
//! the line region is unchanged.

mod common;

use kamitext::{ByteRange, Engine, EngineOptions, Kind, RevealMode, Segment};
use proptest::prelude::*;

fn engine(text: &str) -> Engine {
    Engine::new(
        text,
        EngineOptions {
            reveal: RevealMode::Element,
            ..Default::default()
        },
    )
}

/// Concealed flag of the segment covering byte `at` (queried as a one-byte
/// range so it always lands inside a real segment, never on a boundary).
fn concealed_at(e: &Engine, at: u32) -> bool {
    e.segments_in(ByteRange::new(at, at + 1))[0].concealed
}

// ------------------------------------------------------- inline caret in/out

#[test]
fn inline_caret_inside_reveals_owner_markers() {
    let mut e = engine("plain **bold** text\n");
    e.set_selection(ByteRange::new(9, 9)).unwrap(); // inside "bold"
    assert!(!concealed_at(&e, 6), "lead ** must reveal");
    assert!(!concealed_at(&e, 12), "trail ** must reveal");
}

#[test]
fn inline_caret_outside_conceals_even_on_same_line() {
    // Same physical line as the owner, but outside its byte range: Line
    // mode would reveal this (same line); Element mode must not.
    let mut e = engine("plain **bold** text\n");
    e.set_selection(ByteRange::new(1, 1)).unwrap(); // inside "plain"
    assert!(concealed_at(&e, 6), "lead ** must stay concealed");
    assert!(concealed_at(&e, 12), "trail ** must stay concealed");
}

// --------------------------------------------------------------- boundaries

#[test]
fn boundary_start_activates() {
    let mut e = engine("plain **bold** text\n");
    e.set_selection(ByteRange::new(6, 6)).unwrap(); // exactly owner.start
    assert!(!concealed_at(&e, 6));
    assert!(!concealed_at(&e, 12));
    e.set_selection(ByteRange::new(5, 5)).unwrap(); // one before start
    assert!(concealed_at(&e, 6), "one byte before owner.start must not activate");
}

#[test]
fn boundary_end_activates_including_eof() {
    // Doc ends exactly at the closing `**`: caret at owner.end and EOF
    // simultaneously.
    let mut e = engine("plain **bold**");
    e.set_selection(ByteRange::new(14, 14)).unwrap();
    assert!(!concealed_at(&e, 6));
    assert!(!concealed_at(&e, 12));
}

#[test]
fn boundary_end_activates_at_eol() {
    // Owner ends right before a line break.
    let mut e = engine("**bold**\nnext\n");
    e.set_selection(ByteRange::new(8, 8)).unwrap(); // owner.end, the \n position
    assert!(!concealed_at(&e, 0));
    assert!(!concealed_at(&e, 6));
}

#[test]
fn adjacent_elements_both_activate_at_shared_boundary() {
    let mut e = engine("**a**_b_ tail\n");
    e.set_selection(ByteRange::new(5, 5)).unwrap();
    assert!(!concealed_at(&e, 3), "strong's trailing ** must reveal");
    assert!(!concealed_at(&e, 5), "emphasis's leading _ must reveal");
}

// --------------------------------------------------------------- block scope

#[test]
fn block_markers_stay_line_scoped_inside_element_mode() {
    let text = "# heading\n> quote\n- [ ] task\n";
    let mut e = engine(text);
    // Caret on the heading's line but away from the `#` bytes: reveals `#`
    // (line-scoped), leaves the other lines' block markers concealed.
    e.set_selection(ByteRange::new(5, 5)).unwrap();
    assert!(!concealed_at(&e, 0), "# must reveal: caret is on its line");
    assert!(concealed_at(&e, 10), "> must stay concealed: different line");
    assert!(concealed_at(&e, 19), "task marker must stay concealed: different line");
}

#[test]
fn task_marker_conceals_on_a_lazy_continuation_line() {
    // A list item swallows the following unindented line (CommonMark 5.2
    // lazy continuation), so the item's range covers a line the `- [ ]`
    // doesn't sit on. Block markers conceal per the line they sit on, so a
    // caret down on the continuation must NOT hold the marker revealed.
    let text = "- [ ] some text\nsome new line here\n";
    let mut e = engine(text);

    e.set_selection(ByteRange::new(8, 8)).unwrap(); // line 1, inside "some text"
    assert!(!concealed_at(&e, 0), "task marker reveals: caret is on its line");

    e.set_selection(ByteRange::new(20, 20)).unwrap(); // line 2, the lazy continuation
    assert!(
        concealed_at(&e, 0),
        "task marker must conceal: caret is on a continuation line, not the marker's"
    );
}

#[test]
fn fence_markers_stay_revealed_from_inside_the_block() {
    // The counterpart rule: fenced blocks own their whole range on purpose,
    // so the fences and info string stay legible while editing inside them.
    let text = "```rust\nlet x = 1;\n```\n";
    let mut e = engine(text);
    e.set_selection(ByteRange::new(12, 12)).unwrap(); // inside the code body
    assert!(!concealed_at(&e, 0), "opening fence stays revealed from inside");
    assert!(!concealed_at(&e, 19), "closing fence stays revealed from inside");
}

// ------------------------------------------------------------ nested scopes

#[test]
fn nested_bold_in_heading_scopes_independently() {
    // `#` is line-scoped; `**` is element-scoped — on the same physical
    // line, they can disagree.
    let text = "# **word** extra\n";
    let mut e = engine(text);
    e.set_selection(ByteRange::new(12, 12)).unwrap(); // inside "extra", outside **word**'s owner
    assert!(!concealed_at(&e, 0), "# reveals: caret is on the heading's line");
    assert!(concealed_at(&e, 2), "** must stay concealed: caret is outside its owner");

    e.set_selection(ByteRange::new(5, 5)).unwrap(); // inside "word"
    assert!(!concealed_at(&e, 0), "# still reveals");
    assert!(!concealed_at(&e, 2), "** now reveals: caret is inside its owner");
}

// ----------------------------------------------------------- wide selection

#[test]
fn selection_span_reveals_every_touched_element() {
    let text = "**a** *b* ~~c~~ tail\n";
    let mut e = engine(text);
    e.set_selection(ByteRange::new(2, 12)).unwrap();
    assert!(!concealed_at(&e, 0), "first owner (**a**) must reveal");
    assert!(!concealed_at(&e, 6), "second owner (*b*) must reveal");
    assert!(!concealed_at(&e, 10), "third owner (~~c~~) must reveal");
}

#[test]
fn selection_endpoint_at_owner_end_still_activates() {
    let text = "**a** *b* ~~c~~ tail\n";
    let mut e = engine(text);
    // Selection starts exactly at the first owner's end (byte 5).
    e.set_selection(ByteRange::new(5, 9)).unwrap();
    assert!(!concealed_at(&e, 0), "owner ending exactly at the selection start must activate");
}

// ------------------------------------------------------------- multiline

#[test]
fn multiline_span_reveals_both_delimiters_via_the_same_predicate() {
    // No \n carve-out: a caret anywhere within the owner range activates it,
    // even though the owner itself crosses a line break.
    let mut e = engine("**a\nb** tail\nother\n");
    e.set_selection(ByteRange::new(2, 2)).unwrap(); // line 0, inside owner
    assert!(!concealed_at(&e, 0), "lead ** must reveal");
    assert!(!concealed_at(&e, 5), "trail ** must reveal");

    e.set_selection(ByteRange::new(15, 15)).unwrap(); // line 2, outside owner
    assert!(concealed_at(&e, 0));
    assert!(concealed_at(&e, 5));
}

#[test]
fn within_line_move_into_marker_produces_a_nonempty_patch() {
    // The exact bug shape the selection-aware change detection fixes: the
    // caret moves within one physical line into `**bold**`. The line region
    // never changes, but the reveal outcome does — the old region-only
    // short-circuit would have emitted an empty patch here.
    let mut e = engine("plain **bold** text\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    let p = e.set_selection(ByteRange::new(9, 9)).unwrap();
    assert!(!p.dirty.is_empty(), "caret entering the element must produce a patch");
    let marks: Vec<bool> = e
        .segments_in(ByteRange::new(0, e.len_bytes()))
        .iter()
        .filter(|s| s.kinds.contains(Kind::MARKER))
        .map(|s| s.concealed)
        .collect();
    assert_eq!(marks, vec![false, false]);
}

#[test]
fn empty_patch_when_nothing_flips() {
    // Selection moves but stays inside plain body text, far from any
    // element: even without a region-equality short-circuit, resolve+diff
    // must still emit a genuinely empty patch.
    let mut e = engine("plain body text with **bold** far away\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    let p = e.set_selection(ByteRange::new(3, 3)).unwrap();
    assert!(p.dirty.is_empty(), "moving within plain text must not touch any owner");
}

// ------------------------------------------------------- patch completeness

const SEEDS: &[&str] = &[
    "# h\n**a** *b* ~~c~~ `d` [l](u) ![i](s)\n> q\n- [ ] t\n plain text here\n",
    "**bold**\n*em*\n~~s~~\n`code`\n",
    "plain paragraph one\nplain paragraph two\n**x**_y_\n",
];

proptest! {
    /// Patch completeness (the v0 task-overlay bug shape): for random
    /// selection changes in Element mode, every byte
    /// outside the emitted patch must style identically before and after.
    #[test]
    fn element_mode_selection_patch_is_complete(
        seed in prop::sample::select(SEEDS.to_vec()),
        selections in prop::collection::vec((any::<u32>(), any::<u32>()), 1..8),
    ) {
        let mut e = engine(seed);
        let len = e.len_bytes();
        for (a, b) in selections {
            let old: Vec<Segment> = e.segments_in(ByteRange::new(0, len)).to_vec();
            let a = if len == 0 { 0 } else { a % (len + 1) };
            let b = if len == 0 { 0 } else { b % (len + 1) };
            let patch = e.set_selection(ByteRange::new(a, b)).unwrap();
            let new: Vec<Segment> = e.segments_in(ByteRange::new(0, len)).to_vec();
            // Selection-only change: no bytes move (edit_start/edit_new_len/delta = 0).
            common::assert_patch_sufficient(e.text(), &old, &new, &patch.dirty, 0, 0, 0);
        }
    }
}
