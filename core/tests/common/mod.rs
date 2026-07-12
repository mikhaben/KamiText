//! Shared helpers for the integration-test suites under `core/tests/`. Each
//! consumer declares `mod common;`; since
//! integration tests compile as independent crates, not every consumer uses
//! every item — hence the blanket dead-code allow.

#![allow(dead_code)]

use kamitext::{ByteRange, Engine, Segment};

/// Segment invariants for any engine state: contiguous
/// coverage, no gaps/overlaps, scalar-aligned ranges, UTF-16 continuity and
/// width, and coalesced runs (adjacent segments never share kinds+concealed).
pub fn assert_invariants(e: &Engine) {
    let all = e.segments_in(ByteRange::new(0, e.len_bytes()));
    let text = e.text();
    let mut pos = 0u32;
    let mut u16pos = 0u32;
    for (i, s) in all.iter().enumerate() {
        assert_eq!(s.range.start, pos, "gap/overlap at {i}");
        assert!(s.range.end > s.range.start, "empty segment at {i}");
        assert!(text.is_char_boundary(s.range.start as usize), "scalar split");
        assert!(text.is_char_boundary(s.range.end as usize), "scalar split");
        assert_eq!(s.utf16.start, u16pos, "utf16 continuity at {i}");
        let expect16: u32 = text[s.range.start as usize..s.range.end as usize]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum();
        assert_eq!(s.utf16.end - s.utf16.start, expect16, "utf16 width at {i}");
        if i > 0 {
            let p = &all[i - 1];
            assert!(
                p.kinds != s.kinds || p.concealed != s.concealed,
                "uncoalesced at {i}"
            );
        }
        pos = s.range.end;
        u16pos = s.utf16.end;
    }
    assert_eq!(pos, e.len_bytes(), "coverage");
    assert_eq!(u16pos, e.len_utf16(), "utf16 coverage");
}

/// Building blocks that exercise every marker path when concatenated.
pub const ATOMS: &[&str] = &[
    "**b**", "*i*", "~~s~~", "`c`", "# h\n", "## hh\n", "- li\n", "1. li\n",
    "- [ ] t\n", "- [x] t\n", "> q\n", "```\nx\n```\n", "[l](u)", "![i](s)",
    "---\n", "| a |\n|---|\n| b |\n", "plain ", "日本", "😀", "\u{200D}", "\n",
    "a", " ", "<b>x</b>", "Title\n===\n", "***n***",
];

/// xorshift64* — deterministic, dependency-free PRNG. `pseudo_fuzz.rs` keeps
/// its own copy of this generator because half its ops are deliberately
/// invalid (out-of-bounds, reversed, mid-scalar) — a generation mode
/// `next_valid_op` below does not produce.
pub struct FuzzRng(pub u64);

impl FuzzRng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// A single valid edit or selection op, already resolved against the text it
/// was generated from (offsets scalar-boundary-aligned, always applicable).
#[derive(Debug, Clone)]
pub enum Op {
    Insert { at: u32, s: String },
    Delete { range: ByteRange },
    Replace { range: ByteRange, s: String },
    Select { range: ByteRange },
}

/// Generates one valid, boundary-aligned op against `text`. Factored from the
/// valid-op half of `pseudo_fuzz.rs`'s loop — its deliberately out-of-bounds/
/// reversed/mid-scalar probes are not reproduced here.
pub fn next_valid_op(rng: &mut FuzzRng, text: &str) -> Op {
    let len = text.len() as u64;
    let mut x = rng.below(len + 1) as u32;
    let mut y = rng.below(len + 1) as u32;
    while !text.is_char_boundary(x as usize) {
        x -= 1;
    }
    while !text.is_char_boundary(y as usize) {
        y -= 1;
    }
    let (a, b) = (x.min(y), x.max(y));

    match rng.below(4) {
        0 => Op::Insert {
            at: a,
            s: ATOMS[rng.below(ATOMS.len() as u64) as usize].to_string(),
        },
        1 => Op::Delete {
            range: ByteRange::new(a, b),
        },
        2 => Op::Replace {
            range: ByteRange::new(a, b),
            s: ATOMS[rng.below(ATOMS.len() as u64) as usize].to_string(),
        },
        _ => Op::Select {
            range: ByteRange::new(a, b),
        },
    }
}

/// Patch completeness: every byte outside `dirty` must style
/// identically before and after the change that produced it. `edit_start` /
/// `edit_new_len` / `delta` describe a text-mutating edit — bytes at or after
/// `edit_start + edit_new_len` shifted by `delta = new_len - old_len`; pass
/// zeros for a selection-only change (no bytes move).
pub fn assert_patch_sufficient(
    text: &str,
    old: &[Segment],
    new: &[Segment],
    dirty: &[ByteRange],
    edit_start: u32,
    edit_new_len: u32,
    delta: i64,
) {
    // Segments are sorted/contiguous/non-overlapping (assert_invariants'
    // coverage guarantee), so the covering segment is found by the same
    // partition_point technique as Engine::segments_in — an O(log m) lookup
    // instead of a linear scan, which matters once `text` reaches corpus-doc
    // sizes with thousands of segments.
    let style_of = |segs: &[Segment], pos: u32| {
        let i = segs.partition_point(|s| s.range.end <= pos);
        segs.get(i)
            .filter(|s| s.range.start <= pos)
            .map(|s| (s.kinds, s.concealed))
            .unwrap()
    };
    let in_patch = |pos: u32| dirty.iter().any(|r| r.start <= pos && pos < r.end);
    let len = text.len() as u32;

    for pos in 0..len {
        if !text.is_char_boundary(pos as usize) {
            continue;
        }
        if in_patch(pos) {
            continue;
        }
        let old_pos = if pos >= edit_start + edit_new_len {
            (pos as i64 - delta) as u32
        } else {
            pos
        };
        assert_eq!(
            style_of(old, old_pos),
            style_of(new, pos),
            "styling changed outside the patch at byte {pos}"
        );
    }
}
