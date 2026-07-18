//! Golden tests: hand-reviewed segment/element/patch expectations covering
//! the fixture list and the composition examples.

mod common;

use kamitext::{
    ByteRange, ElementKind, Engine, EngineOptions, Extensions, KamiError, Kind, RevealMode,
};

fn engine(text: &str) -> Engine {
    Engine::new(text, EngineOptions::default())
}

/// Renders segments as compact strings: `start..end kinds [conc]`.
fn segs(e: &Engine) -> Vec<String> {
    e.segments_in(ByteRange::new(0, e.len_bytes()))
        .iter()
        .map(|s| {
            let kinds = kind_names(s.kinds);
            let conc = if s.concealed { " conc" } else { "" };
            format!(
                "{}..{} u16:{}..{} {}{}",
                s.range.start, s.range.end, s.utf16.start, s.utf16.end, kinds, conc
            )
        })
        .collect()
}

fn kind_names(k: Kind) -> String {
    const NAMES: &[(Kind, &str)] = &[
        (Kind::BODY, "BODY"),
        (Kind::HEADING1, "H1"),
        (Kind::HEADING2, "H2"),
        (Kind::HEADING3, "H3"),
        (Kind::HEADING4, "H4"),
        (Kind::HEADING5, "H5"),
        (Kind::HEADING6, "H6"),
        (Kind::STRONG, "STRONG"),
        (Kind::EMPHASIS, "EM"),
        (Kind::STRIKETHROUGH, "STRIKE"),
        (Kind::CODE_SPAN, "CODESPAN"),
        (Kind::CODE_BLOCK, "CODEBLOCK"),
        (Kind::FENCE_INFO, "FENCEINFO"),
        (Kind::BLOCKQUOTE, "QUOTE"),
        (Kind::LIST_BULLET, "BULLET"),
        (Kind::LIST_ORDINAL, "ORDINAL"),
        (Kind::TASK_MARKER, "TASK"),
        (Kind::LINK, "LINK"),
        (Kind::IMAGE, "IMAGE"),
        (Kind::TABLE, "TABLE"),
        (Kind::THEMATIC_BREAK, "HR"),
        (Kind::MARKER, "MARK"),
        (Kind::HTML_RAW, "HTML"),
    ];
    let mut parts = Vec::new();
    for (bit, name) in NAMES {
        if k.contains(*bit) {
            parts.push(*name);
        }
    }
    parts.join("|")
}

// ---------------------------------------------------------------- reveal off

/// With the caret parked on line 0, markers on other lines are concealed.

#[test]
fn composition_example_revealed_at_caret() {
    // Caret starts at 0..0 → line 0 revealed → all markers visible.
    let e = engine("# **word**");
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 MARK",
            "2..4 u16:2..4 H1|MARK",
            "4..8 u16:4..8 H1|STRONG",
            "8..10 u16:8..10 H1|MARK",
        ]
    );
}

#[test]
fn composition_example_concealed_away_from_caret() {
    let mut e = engine("plain\n# **word**");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..6 u16:0..6 BODY",
            "6..8 u16:6..8 MARK conc",
            "8..10 u16:8..10 H1|MARK conc",
            "10..14 u16:10..14 H1|STRONG",
            "14..16 u16:14..16 H1|MARK conc",
        ]
    );
}

#[test]
fn reveal_mode_none_conceals_everything() {
    let e = Engine::new(
        "# **word**",
        EngineOptions {
            reveal: RevealMode::None,
            ..Default::default()
        },
    );
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 MARK conc",
            "2..4 u16:2..4 H1|MARK conc",
            "4..8 u16:4..8 H1|STRONG",
            "8..10 u16:8..10 H1|MARK conc",
        ]
    );
}

#[test]
fn nested_emphasis_triple_star() {
    let mut e = engine("x\n***x***");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..3 u16:2..3 MARK conc",
            "3..5 u16:3..5 EM|MARK conc",
            "5..6 u16:5..6 STRONG|EM",
            "6..8 u16:6..8 EM|MARK conc",
            "8..9 u16:8..9 MARK conc",
        ]
    );
}

#[test]
fn nested_emphasis_strong_underscore() {
    let e = engine("**_x_**");
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 MARK",
            "2..3 u16:2..3 STRONG|MARK",
            "3..4 u16:3..4 STRONG|EM",
            "4..5 u16:4..5 STRONG|MARK",
            "5..7 u16:5..7 MARK",
        ]
    );
}

#[test]
fn multi_backtick_code_span() {
    let e = engine("`` a`b ``");
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 MARK",
            "2..7 u16:2..7 CODESPAN",
            "7..9 u16:7..9 MARK",
        ]
    );
}

#[test]
fn emoji_astral_mid_marker() {
    // Astral scalar (4 bytes / 2 UTF-16 units) inside strong text.
    let e = engine("**a😀b**");
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 MARK",
            "2..8 u16:2..6 STRONG", // a😀b = 1+2+1 UTF-16 units over 6 bytes
            "8..10 u16:6..8 MARK",
        ]
    );
}

#[test]
fn zwj_sequence_in_strong() {
    let family = "\u{1F468}\u{200D}\u{1F469}"; // 👨‍👩: 4+3+4 bytes, 2+1+2 units
    let text = format!("**{family}**");
    let e = engine(&text);
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 MARK",
            "2..13 u16:2..7 STRONG",
            "13..15 u16:7..9 MARK",
        ]
    );
}

#[test]
fn cjk_heading() {
    let e = engine("# 日本語");
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec!["0..2 u16:0..2 MARK", "2..11 u16:2..5 H1"]
    );
}

#[test]
fn empty_document() {
    let e = engine("");
    common::assert_invariants(&e);
    assert!(segs(&e).is_empty());
    assert_eq!(e.len_bytes(), 0);
    assert_eq!(e.len_utf16(), 0);
}

#[test]
fn no_trailing_newline() {
    let e = engine("plain text");
    common::assert_invariants(&e);
    assert_eq!(segs(&e), vec!["0..10 u16:0..10 BODY"]);
}

#[test]
fn task_list_marker_and_elements() {
    let mut e = engine("- [ ] todo\n- [x] done\n");
    e.set_selection(ByteRange::new(22, 22)).unwrap(); // trailing empty line
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..6 u16:0..6 TASK conc",
            "6..11 u16:6..11 BODY",
            "11..17 u16:11..17 TASK conc",
            "17..22 u16:17..22 BODY",
        ]
    );
    let els = e.elements_in(ByteRange::new(0, e.len_bytes()));
    assert_eq!(els.len(), 2);
    assert_eq!(els[0].id, 0);
    assert_eq!(els[0].range, ByteRange::new(0, 11));
    assert_eq!(els[0].kind, ElementKind::Task { checked: false });
    assert_eq!(els[1].id, 1);
    assert_eq!(els[1].range, ByteRange::new(11, 22));
    assert_eq!(els[1].kind, ElementKind::Task { checked: true });
}

#[test]
fn bullet_list_never_concealed() {
    let mut e = engine("x\n- one\n- two\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    // Bullets stay visible even away from the caret (conceal class Never).
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..3 u16:2..3 BULLET",
            "3..8 u16:3..8 BODY",
            "8..9 u16:8..9 BULLET",
            "9..14 u16:9..14 BODY",
        ]
    );
}

#[test]
fn ordered_list_ordinal() {
    let e = engine("1. one\n2. two\n");
    common::assert_invariants(&e);
    let s = segs(&e);
    assert_eq!(s[0], "0..2 u16:0..2 ORDINAL");
    assert_eq!(s[2], "7..9 u16:7..9 ORDINAL");
}

#[test]
fn link_segments_and_element() {
    let mut e = engine("x\n[text](https://e.com)");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..3 u16:2..3 MARK conc",
            "3..7 u16:3..7 LINK",
            "7..23 u16:7..23 MARK conc",
        ]
    );
    let els = e.elements_in(ByteRange::new(2, 23));
    assert_eq!(els.len(), 1);
    assert_eq!(els[0].range, ByteRange::new(2, 23));
    assert_eq!(
        els[0].kind,
        ElementKind::Link {
            dest: ByteRange::new(9, 22)
        }
    );
    assert_eq!(&e.text()[9..22], "https://e.com");
}

#[test]
fn image_element() {
    let e = engine("![alt](img.png)");
    let els = e.elements_in(ByteRange::new(0, e.len_bytes()));
    assert_eq!(els.len(), 1);
    assert_eq!(
        els[0].kind,
        ElementKind::Image {
            src: ByteRange::new(7, 14)
        }
    );
    assert_eq!(&e.text()[7..14], "img.png");
}

#[test]
fn wikilink_plain_segments_and_element() {
    let mut e = engine("x\n[[Note]]");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..4 u16:2..4 MARK conc",
            "4..8 u16:4..8 LINK",
            "8..10 u16:8..10 MARK conc",
        ]
    );
    let els = e.elements_in(ByteRange::new(2, 10));
    assert_eq!(els.len(), 1);
    assert_eq!(els[0].range, ByteRange::new(2, 10));
    assert_eq!(
        els[0].kind,
        ElementKind::WikiLink {
            target: ByteRange::new(4, 8)
        }
    );
    assert_eq!(&e.text()[4..8], "Note");
}

#[test]
fn wikilink_piped_target_excludes_alias() {
    let mut e = engine("x\n[[target|alias]]");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    // Lead marker swallows `[[target|`; only the alias stays visible as LINK.
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..11 u16:2..11 MARK conc",
            "11..16 u16:11..16 LINK",
            "16..18 u16:16..18 MARK conc",
        ]
    );
    let els = e.elements_in(ByteRange::new(2, 18));
    assert_eq!(els.len(), 1);
    assert_eq!(
        els[0].kind,
        ElementKind::WikiLink {
            target: ByteRange::new(4, 10)
        }
    );
    assert_eq!(&e.text()[4..10], "target");
}

#[test]
fn wikilink_reveals_at_caret() {
    // Caret at 0 → line 0 revealed → both `[[` and `]]` visible.
    let e = engine("[[Note]]");
    common::assert_invariants(&e);
    let marks: Vec<bool> = e
        .segments_in(ByteRange::new(0, e.len_bytes()))
        .iter()
        .filter(|s| s.kinds.contains(Kind::MARKER))
        .map(|s| s.concealed)
        .collect();
    assert_eq!(marks, vec![false, false]);
}

#[test]
fn wikilinks_disabled_are_plain_text() {
    let opts = EngineOptions {
        extensions: Extensions::empty(),
        ..Default::default()
    };
    let e = Engine::new("[[Note]]", opts);
    let all = e.segments_in(ByteRange::new(0, e.len_bytes()));
    assert!(!all.iter().any(|s| s.kinds.contains(Kind::LINK)));
    assert!(!all.iter().any(|s| s.kinds.contains(Kind::MARKER)));
    assert!(e.elements_in(ByteRange::new(0, e.len_bytes())).is_empty());
}

#[test]
fn fenced_code_block() {
    let mut e = engine("x\n```rust\nfn a() {}\n```\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..5 u16:2..5 MARK conc",
            "5..9 u16:5..9 FENCEINFO conc",
            "9..20 u16:9..20 CODEBLOCK",
            "20..23 u16:20..23 MARK conc",
            "23..24 u16:23..24 BODY",
        ]
    );
    let els = e.elements_in(ByteRange::new(2, 23));
    assert_eq!(els.len(), 1);
    assert_eq!(
        els[0].kind,
        ElementKind::Fence {
            info: ByteRange::new(5, 9)
        }
    );
}

#[test]
fn blockquote_markers_per_line() {
    let mut e = engine("x\n> quoted\n> more\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    // `> ` markers compose QUOTE|MARK: the quote bit reaches the paragraph's
    // first character so a theme's quote paragraph style can key off it (the
    // TASK-marker pattern), while the marker owner still conceals off-line.
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..4 u16:2..4 QUOTE|MARK conc",
            "4..11 u16:4..11 QUOTE",
            "11..13 u16:11..13 QUOTE|MARK conc",
            "13..18 u16:13..18 QUOTE",
        ]
    );
}

#[test]
fn table_single_kind() {
    let mut e = engine("x\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    // The table body composes TABLE|MARKER (a single Block-scoped marker over
    // the trimmed range — hosts draw a grid in its place while concealed); the
    // trailing newline stays plain TABLE (same convention as headings).
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..31 u16:2..31 TABLE|MARK conc",
            "31..32 u16:31..32 TABLE",
        ]
    );
}

#[test]
fn table_reveals_whole_block_with_caret_inside() {
    let mut e = engine("x\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    e.set_selection(ByteRange::new(24, 24)).unwrap(); // caret on the LAST table line
    common::assert_invariants(&e);
    // One owner spans every table line, so a caret on ANY of them reveals the
    // ENTIRE raw table — the tap-to-edit contract hosts rely on.
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..31 u16:2..31 TABLE|MARK",
            "31..32 u16:31..32 TABLE",
        ]
    );
}

#[test]
fn quoted_table_stays_raw_with_disjoint_markers() {
    // A table inside a blockquote gets NO whole-table marker: the quote's
    // per-line `> ` markers live inside the table's range, and markers must
    // stay disjoint. The quoted table renders raw (host draws no grid either).
    let mut e = engine("> | a | b |\n> |---|---|\n> | 1 | 2 |\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    common::assert_invariants(&e);
    let segments = segs(&e);
    assert!(
        segments.iter().any(|s| s.contains("QUOTE|TABLE") && !s.contains("conc")),
        "table content must stay visible inside a quote: {segments:?}"
    );
}

#[test]
fn table_suppresses_inline_markers_in_cells() {
    let mut e = engine("| **b** | `c` |\n|---|---|\n| [l](u) | [[w]] |\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap(); // caret inside → revealed
    common::assert_invariants(&e);
    // Inline markers/elements are suppressed inside tables (markers must be
    // disjoint; the whole-table marker owns conceal). Content kinds still
    // compose so a revealed table keeps its styling; no link/image/wikilink
    // elements are registered from cells.
    let segments = segs(&e);
    assert!(segments.iter().all(|s| !s.contains("conc")), "revealed: {segments:?}");
    assert!(segments.iter().any(|s| s.contains("STRONG|TABLE")), "{segments:?}");
    assert!(segments.iter().any(|s| s.contains("CODESPAN|TABLE")), "{segments:?}");
    assert_eq!(e.elements_in(ByteRange::new(0, 45)).len(), 0);
}

#[test]
fn thematic_break() {
    // The rule line carries a co-located Block-scoped marker: revealed (raw
    // `---`) while the caret's line intersects it, concealed otherwise so a
    // host can draw a divider in its place.
    let e = engine("---\n");
    common::assert_invariants(&e);
    assert_eq!(segs(&e), vec!["0..3 u16:0..3 HR|MARK", "3..4 u16:3..4 BODY"]);
}

#[test]
fn thematic_break_conceals_off_line() {
    let mut e = engine("---\nbody\n");
    e.set_selection(ByteRange::new(5, 5)).unwrap(); // caret on the body line
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec!["0..3 u16:0..3 HR|MARK conc", "3..9 u16:3..9 BODY"]
    );
}

#[test]
fn setext_heading() {
    let e = engine("Title\n=====\n");
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec!["0..6 u16:0..6 H1", "6..11 u16:6..11 MARK", "11..12 u16:11..12 H1"]
    );
}

#[test]
fn html_raw() {
    let e = engine("a <b>bold</b> c");
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..2 u16:0..2 BODY",
            "2..5 u16:2..5 HTML",
            "5..9 u16:5..9 BODY",
            "9..13 u16:9..13 HTML",
            "13..15 u16:13..15 BODY",
        ]
    );
}

// ------------------------------------------------------------ reveal policy

#[test]
fn multiline_span_reveals_both_delimiters() {
    // Strong spanning two lines: caret on either line reveals both `**`.
    let text = "**a\nb** tail\nother";
    let mut e = engine(text);
    for caret in [0u32, 5] {
        e.set_selection(ByteRange::new(caret, caret)).unwrap();
        let all = e.segments_in(ByteRange::new(0, e.len_bytes()));
        let marks: Vec<bool> = all
            .iter()
            .filter(|s| s.kinds.contains(Kind::MARKER))
            .map(|s| s.concealed)
            .collect();
        assert_eq!(marks, vec![false, false], "caret {caret}");
    }
    // Caret on the third line conceals both.
    e.set_selection(ByteRange::new(14, 14)).unwrap();
    let all = e.segments_in(ByteRange::new(0, e.len_bytes()));
    let marks: Vec<bool> = all
        .iter()
        .filter(|s| s.kinds.contains(Kind::MARKER))
        .map(|s| s.concealed)
        .collect();
    assert_eq!(marks, vec![true, true]);
}

#[test]
fn selection_spanning_multiple_lines_reveals_union() {
    let text = "# one\n**two**\n*three*\nplain";
    let mut e = engine(text);
    // Selection covering lines 1-2 reveals their markers, not line 0's.
    e.set_selection(ByteRange::new(8, 16)).unwrap();
    common::assert_invariants(&e);
    let all = e.segments_in(ByteRange::new(0, e.len_bytes()));
    for s in all {
        if s.kinds.intersects(Kind::MARKER) {
            let on_line0 = s.range.end <= 6;
            assert_eq!(s.concealed, on_line0, "segment {:?}", s.range);
        }
    }
}

#[test]
fn selection_direction_independent() {
    let text = "# one\n**two**\n";
    let mut e1 = engine(text);
    let mut e2 = engine(text);
    e1.set_selection(ByteRange::new(2, 9)).unwrap();
    e2.set_selection(ByteRange::new(9, 2)).unwrap();
    assert_eq!(
        e1.segments_in(ByteRange::new(0, e1.len_bytes())),
        e2.segments_in(ByteRange::new(0, e2.len_bytes()))
    );
}

// ----------------------------------------------------------------- patches

#[test]
fn empty_patch_on_noop_selection_change() {
    let mut e = engine("# one\ntwo three\n");
    e.set_selection(ByteRange::new(7, 7)).unwrap();
    // Moving within the same line: reveal region unchanged ⇒ empty patch.
    let p = e.set_selection(ByteRange::new(10, 10)).unwrap();
    assert!(p.dirty.is_empty());
}

#[test]
fn selection_patch_covers_conceal_flips() {
    let mut e = engine("# one\n**two**\n");
    e.set_selection(ByteRange::new(0, 0)).unwrap();
    let p = e.set_selection(ByteRange::new(7, 7)).unwrap();
    // Line 0 markers conceal, line 1 markers reveal: both lines dirty.
    assert!(!p.dirty.is_empty());
    let covers = |pos: u32| p.dirty.iter().any(|r| r.start <= pos && pos < r.end);
    assert!(covers(0), "heading marker flip must be dirty");
    assert!(covers(6), "strong marker flip must be dirty");
    // Re-fetching only dirty ranges must reproduce the new state (§4.3-4):
    // verify patch ranges are segment-aligned.
    for r in &p.dirty {
        let sl = e.segments_in(*r);
        assert_eq!(sl.first().unwrap().range.start, r.start);
        assert_eq!(sl.last().unwrap().range.end, r.end);
    }
}

#[test]
fn edit_patch_is_segment_aligned_and_minimal() {
    let mut e = engine("aaaa bbbb cccc\n# unrelated heading\n");
    let p = e.apply_edit(ByteRange::new(5, 9), "XXXX").unwrap();
    assert_eq!(p.dirty.len(), 1);
    let d = p.dirty[0];
    // The dirty range must cover the edit and be segment-aligned.
    assert!(d.start <= 5 && d.end >= 9);
    let sl = e.segments_in(d);
    assert_eq!(sl.first().unwrap().range.start, d.start);
    assert_eq!(sl.last().unwrap().range.end, d.end);
    // The heading on line 2 did not change: dirty must not extend past line 1.
    assert!(d.end <= 15, "dirty {d:?} leaked into unchanged suffix");
}

#[test]
fn block_split_edit_typing_fence_above_content() {
    // Typing ``` above content converts the paragraph into a code block.
    let mut e = engine("\nplain paragraph\n");
    let p = e.apply_edit(ByteRange::new(0, 0), "```").unwrap();
    assert_eq!(e.text(), "```\nplain paragraph\n");
    common::assert_invariants(&e);
    let all = e.segments_in(ByteRange::new(0, e.len_bytes()));
    assert!(
        all.iter().any(|s| s.kinds.contains(Kind::CODE_BLOCK)),
        "content must restyle as code"
    );
    // Everything from the fence through the absorbed paragraph is dirty.
    assert!(!p.dirty.is_empty());
    assert_eq!(p.dirty[0].start, 0);
    assert!(p.dirty[0].end >= 19);
}

#[test]
fn block_merge_edit_deleting_blank_line() {
    let mut e = engine("para one\n\npara two\n");
    let p = e.apply_edit(ByteRange::new(9, 10), "").unwrap();
    assert_eq!(e.text(), "para one\npara two\n");
    common::assert_invariants(&e);
    assert!(!p.dirty.is_empty());
}

#[test]
fn edit_dirty_ranges_sorted_coalesced() {
    let mut e = engine("# a\n**b**\n*c*\n");
    let p = e.apply_edit(ByteRange::new(4, 4), "x").unwrap();
    for w in p.dirty.windows(2) {
        assert!(w[0].end < w[1].start, "unsorted/uncoalesced patch");
    }
}

// ------------------------------------------------------------- validation

#[test]
fn invalid_ranges_rejected_without_mutation() {
    let mut e = engine("a😀b");
    let before: Vec<_> = segs(&e);
    // Out of bounds.
    assert_eq!(
        e.apply_edit(ByteRange::new(0, 99), "x"),
        Err(KamiError::InvalidRange)
    );
    // Scalar split (byte 2 is inside the emoji).
    assert_eq!(
        e.apply_edit(ByteRange::new(2, 3), "x"),
        Err(KamiError::InvalidRange)
    );
    // Reversed range.
    assert_eq!(
        e.apply_edit(ByteRange::new(3, 1), "x"),
        Err(KamiError::InvalidRange)
    );
    assert_eq!(
        e.set_selection(ByteRange::new(0, 99)),
        Err(KamiError::InvalidRange)
    );
    assert_eq!(e.text(), "a😀b");
    assert_eq!(segs(&e), before);
}

#[test]
fn extensions_toggle_off() {
    let opts = EngineOptions {
        extensions: Extensions::empty(),
        ..Default::default()
    };
    let e = Engine::new("~~gone~~ and\n- [ ] task\n", opts);
    let all = e.segments_in(ByteRange::new(0, e.len_bytes()));
    assert!(!all.iter().any(|s| s.kinds.contains(Kind::STRIKETHROUGH)));
    assert!(!all.iter().any(|s| s.kinds.contains(Kind::TASK_MARKER)));
    // The bullet is still a list bullet.
    assert!(all.iter().any(|s| s.kinds.contains(Kind::LIST_BULLET)));
}

// -------------------------------------------------------------- behaviors

#[test]
fn newline_continues_bullet_list() {
    let e = engine("- one");
    let plan = e.newline_plan(5).unwrap().unwrap();
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.edits[0].0, ByteRange::new(5, 5));
    assert_eq!(plan.edits[0].1, "\n- ");
    assert_eq!(plan.caret, 8);
}

#[test]
fn newline_continues_ordered_list_incrementing() {
    let e = engine("1. one");
    let plan = e.newline_plan(6).unwrap().unwrap();
    assert_eq!(plan.edits[0].1, "\n2. ");
}

#[test]
fn newline_continues_task() {
    let e = engine("- [x] done");
    let plan = e.newline_plan(10).unwrap().unwrap();
    assert_eq!(plan.edits[0].1, "\n- [ ] ");
}

#[test]
fn newline_continues_quote() {
    let e = engine("> quoted");
    let plan = e.newline_plan(8).unwrap().unwrap();
    assert_eq!(plan.edits[0].1, "\n> ");
}

#[test]
fn newline_exits_empty_item() {
    let e = engine("- one\n- ");
    let plan = e.newline_plan(8).unwrap().unwrap();
    assert_eq!(plan.edits[0].0, ByteRange::new(6, 8));
    assert_eq!(plan.edits[0].1, "");
    assert_eq!(plan.caret, 6);
}

#[test]
fn newline_plain_paragraph_is_none() {
    let e = engine("plain text");
    assert_eq!(e.newline_plan(5).unwrap(), None);
}

#[test]
fn newline_inside_code_block_is_none() {
    let e = engine("```\n- not a list\n```\n");
    assert_eq!(e.newline_plan(10).unwrap(), None);
}

#[test]
fn newline_misaligned_offset_is_error() {
    let e = engine("a😀b");
    assert_eq!(e.newline_plan(2), Err(KamiError::InvalidRange));
    assert_eq!(e.toggle_task_plan(99), Err(KamiError::InvalidRange));
}

#[test]
fn toggle_task_flips_box() {
    let e = engine("- [ ] todo\n- [x] done\n");
    let plan = e.toggle_task_plan(7).unwrap().unwrap();
    assert_eq!(plan.edits[0].0, ByteRange::new(2, 5));
    assert_eq!(plan.edits[0].1, "[x]");
    assert_eq!(plan.caret, 7);

    let plan = e.toggle_task_plan(15).unwrap().unwrap();
    assert_eq!(plan.edits[0].0, ByteRange::new(13, 16));
    assert_eq!(plan.edits[0].1, "[ ]");

    assert!(e.toggle_task_plan(0).unwrap().is_some());
    let none = engine("plain").toggle_task_plan(2).unwrap();
    assert_eq!(none, None);
}

// ------------------------------------------------------- offset conversion

#[test]
fn byte_utf16_round_trips() {
    let e = engine("a😀日**x**\n");
    // a=1/1, 😀=4/2, 日=3/1
    assert_eq!(e.byte_to_utf16(0), 0);
    assert_eq!(e.byte_to_utf16(1), 1);
    assert_eq!(e.byte_to_utf16(5), 3);
    assert_eq!(e.byte_to_utf16(8), 4);
    assert_eq!(e.utf16_to_byte(3), 5);
    // Mid-surrogate rounds down to the scalar start.
    assert_eq!(e.utf16_to_byte(2), 1);
    assert_eq!(e.len_utf16(), e.byte_to_utf16(e.len_bytes()));
}

// --------------------------------------------------------------- queries

#[test]
fn segments_in_subrange_is_contiguous_cover() {
    let e = engine("# **word** tail\n");
    // Query inside "word": returns the containing segment (unclipped).
    let sl = e.segments_in(ByteRange::new(5, 6));
    assert_eq!(sl.len(), 1);
    assert_eq!(sl[0].range, ByteRange::new(4, 8));
    // Zero-width query at a boundary: the segment starting there.
    let sl = e.segments_in(ByteRange::new(4, 4));
    assert_eq!(sl.len(), 1);
    assert_eq!(sl[0].range, ByteRange::new(4, 8));
    // Query at end of doc (zero-width): empty.
    let sl = e.segments_in(ByteRange::new(e.len_bytes(), e.len_bytes()));
    assert!(sl.is_empty());
}

#[test]
fn wikilink_empty_alias_conceals_whole_node() {
    // `[[a|]]` (piped, empty alias) is a live mid-typing state that makes
    // pulldown 0.13 re-emit the paragraph's tail inside the still-open link
    // (probed). Regression for the reversed-marker panic / release-mode
    // segment corruption: the node conceals whole, the tail styles normally,
    // and the element still carries the target.
    let mut e = engine("[[a|]] tail\nx\n");
    e.set_selection(ByteRange::new(12, 12)).unwrap();
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec!["0..6 u16:0..6 MARK conc", "6..14 u16:6..14 BODY"]
    );
    let els = e.elements_in(ByteRange::new(0, 6));
    assert_eq!(els.len(), 1);
    assert_eq!(
        els[0].kind,
        ElementKind::WikiLink {
            target: ByteRange::new(2, 3)
        }
    );
    assert_eq!(&e.text()[2..3], "a");
}

#[test]
fn wikilink_empty_alias_trailing_siblings_stay_wellformed() {
    // After the empty-alias quirk, pulldown re-walks the paragraph tail with
    // corrupted state: a later healthy `[[Real|alias]]` arrives as a plain
    // Link. Rendering must stay correct (markers + visible alias) and every
    // invariant must hold; only that element's kind degrades until the
    // `[[a|]]` upstream is completed.
    let mut e = engine("[[a|]] mid [[Real|alias]] end\nx\n");
    e.set_selection(ByteRange::new(30, 30)).unwrap();
    common::assert_invariants(&e);
    assert_eq!(
        segs(&e),
        vec![
            "0..6 u16:0..6 MARK conc",
            "6..11 u16:6..11 BODY",
            "11..18 u16:11..18 MARK conc",
            "18..23 u16:18..23 LINK",
            "23..25 u16:23..25 MARK conc",
            "25..32 u16:25..32 BODY",
        ]
    );
    assert_eq!(&e.text()[18..23], "alias");
    let els = e.elements_in(ByteRange::new(0, 6));
    assert_eq!(els.len(), 1);
    assert_eq!(
        els[0].kind,
        ElementKind::WikiLink {
            target: ByteRange::new(2, 3)
        }
    );
}

#[test]
fn wikilink_in_table_cell_paints_body_only() {
    // Inside a table, inline markers/elements are suppressed: the whole-table
    // Block marker owns conceal, and the wikilink body paints LINK so the
    // concealed grid renderer can style it.
    let mut e = engine("| [[w]] |\n|---|\n| b |\nx\n");
    e.set_selection(ByteRange::new(22, 22)).unwrap();
    common::assert_invariants(&e);
    let els = e.elements_in(ByteRange::new(0, e.len_bytes()));
    assert!(
        els.iter().all(|el| !matches!(el.kind, ElementKind::WikiLink { .. })),
        "in-table wikilink must not emit an element"
    );
    let link_seg = segs(&e).iter().any(|s| s.contains("LINK"));
    assert!(link_seg, "wikilink body inside the table paints LINK");
}

#[test]
fn single_char_reference_link_keeps_visible_label() {
    // Critic-caught regression guard: `[1]` (numeric citation, reference
    // style) must keep its "1" visible off-caret — a closer-offset body
    // filter once concealed the whole node.
    let mut e = engine("[1] cite\nx\n\n[1]: http://e.com\n");
    e.set_selection(ByteRange::new(9, 9)).unwrap();
    common::assert_invariants(&e);
    let all = segs(&e);
    assert!(
        all.iter().any(|s| s.starts_with("1..2 ") && s.contains("LINK") && !s.contains("conc")),
        "the 1-char label must stay visible: {all:?}"
    );
}

#[test]
fn quoted_thematic_break_stays_raw() {
    // v1 rule, same as quoted tables: inside a blockquote the rule emits no
    // Block marker, so `> ---` shows its raw source instead of concealing
    // into a row the host has no pinned height to draw a divider into.
    let mut e = engine("> ---\nx\n");
    e.set_selection(ByteRange::new(6, 6)).unwrap();
    common::assert_invariants(&e);
    let all = segs(&e);
    assert!(
        all.iter().any(|s| s.contains("QUOTE|HR") && !s.contains("conc")),
        "quoted HR body stays visible: {all:?}"
    );
    // A top-level rule still conceals away from the caret.
    let mut top = engine("---\nx\n");
    top.set_selection(ByteRange::new(4, 4)).unwrap();
    assert!(segs(&top).iter().any(|s| s.contains("HR") && s.contains("conc")));
}

#[test]
fn heading_elements_atx_setext_fenced() {
    let src = "# One\n\nTitle\n=====\n\n## Two ##\n\n```\n# not a heading\n```\n";
    let e = engine(src);
    let heads: Vec<(u8, &str)> = e
        .elements_in(ByteRange::new(0, src.len() as u32))
        .iter()
        .filter_map(|el| match el.kind {
            ElementKind::Heading { level, text } => {
                Some((level, &src[text.start as usize..text.end as usize]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(heads, vec![(1, "One"), (1, "Title"), (2, "Two")]);
}

#[test]
fn heading_element_empty_atx_has_zero_width_text() {
    let src = "##\n";
    let e = engine(src);
    let els = e.elements_in(ByteRange::new(0, src.len() as u32));
    let heading = els
        .iter()
        .find_map(|el| match el.kind {
            ElementKind::Heading { level, text } => Some((level, text)),
            _ => None,
        })
        .expect("empty ATX heading emits an element");
    assert_eq!(heading.0, 2);
    assert_eq!(heading.1.start, heading.1.end);
}

#[test]
fn heading_element_title_trims_surrounding_whitespace() {
    // The opening marker consumes one space; extra title padding is trimmed
    // from the element's text range (the paint keeps it — display-only).
    let src = "#   Spaced Title  \n";
    let e = engine(src);
    let heads: Vec<(u8, &str)> = e
        .elements_in(ByteRange::new(0, src.len() as u32))
        .iter()
        .filter_map(|el| match el.kind {
            ElementKind::Heading { level, text } => {
                Some((level, &src[text.start as usize..text.end as usize]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(heads, vec![(1, "Spaced Title")]);
}

#[test]
fn heading_element_titles_are_crlf_safe() {
    // CRLF documents: no stray `\r` in titles, a closing `#`-run is still
    // recognized (and stripped) before the `\r`, and an empty `##\r\n` ATX
    // heading stays ATX (zero-width title) instead of leaking "##\r".
    let src = "# One\r\n\r\nTitle\r\n=====\r\n\r\n## Two ##\r\n\r\n##\r\n";
    let e = engine(src);
    let heads: Vec<(u8, &str)> = e
        .elements_in(ByteRange::new(0, src.len() as u32))
        .iter()
        .filter_map(|el| match el.kind {
            ElementKind::Heading { level, text } => {
                Some((level, &src[text.start as usize..text.end as usize]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(heads, vec![(1, "One"), (1, "Title"), (2, "Two"), (2, "")]);
}

#[test]
fn heading_element_setext_in_blockquote_excludes_quote_prefix() {
    // The underline line's `> ` prefix sits between the title and the marker
    // run; it must not leak into the title range.
    let src = "> Title\n> =====\n";
    let e = engine(src);
    let heads: Vec<(u8, &str)> = e
        .elements_in(ByteRange::new(0, src.len() as u32))
        .iter()
        .filter_map(|el| match el.kind {
            ElementKind::Heading { level, text } => {
                Some((level, &src[text.start as usize..text.end as usize]))
            }
            _ => None,
        })
        .collect();
    assert_eq!(heads, vec![(1, "Title")]);
}
