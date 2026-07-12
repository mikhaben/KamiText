//! Segment flattening: paints + markers → sorted, non-overlapping, covering,
//! coalesced raw segments.
//!
//! Raw segments are conceal-agnostic: marker segments carry their owner range
//! so the conceal pass can resolve them against the reveal region without
//! reparsing.

use crate::parse::ParseOut;
use crate::types::{ByteRange, Kind, MarkerScope, Utf16Range};

/// A flattened segment before conceal resolution. `owner` and `scope` are
/// `Some` together for marker bytes (`owner` is the syntactic span the
/// marker belongs to; `scope` is carried from the marker's `push_marker`
/// site).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawSegment {
    pub range: ByteRange,
    pub utf16: Utf16Range,
    pub kinds: Kind,
    pub owner: Option<ByteRange>,
    pub scope: Option<MarkerScope>,
}

/// Sweep event packed into a u64 for fast integer sorting:
/// `pos` in bits 63–32, phase in bit 31 (0 = end, 1 = start, so ends sort
/// before starts at equal positions and half-open ranges never overlap
/// transiently), payload index in bits 30–0 (into paints when
/// `idx < marker_base`, else into markers).
type Event = u64;

const PHASE_START: u64 = 1 << 31;
const IDX_MASK: u64 = PHASE_START - 1;

#[inline]
fn pack(pos: u32, start: bool, idx: u32) -> Event {
    ((pos as u64) << 32) | (u64::from(start) << 31) | idx as u64
}

#[derive(Default)]
pub struct FlattenScratch {
    events: Vec<Event>,
    active_markers: Vec<u32>,
}

pub fn flatten(
    parse: &ParseOut,
    len: u32,
    scratch: &mut FlattenScratch,
    out: &mut Vec<RawSegment>,
) {
    out.clear();
    let events = &mut scratch.events;
    events.clear();
    let marker_base = parse.paints.len() as u32;
    for (i, p) in parse.paints.iter().enumerate() {
        if !p.range.is_empty() {
            events.push(pack(p.range.start, true, i as u32));
            events.push(pack(p.range.end, false, i as u32));
        }
    }
    for (i, m) in parse.markers.iter().enumerate() {
        debug_assert!(!m.range.is_empty());
        events.push(pack(m.range.start, true, marker_base + i as u32));
        events.push(pack(m.range.end, false, marker_base + i as u32));
    }
    // Plain integer sort keeps ties deterministic (idx is part of the key);
    // counts and bit-unions are commutative, so tie order among same-position
    // same-phase events cannot affect the output.
    events.sort_unstable();

    // Per-kind-bit nesting counts (e.g. emphasis inside emphasis).
    let mut counts = [0u16; 64];
    let mut kinds = Kind::empty();
    let active = &mut scratch.active_markers;
    active.clear();

    let mut pos = 0u32;
    let mut ev_i = 0;
    while ev_i < events.len() {
        let boundary = (events[ev_i] >> 32) as u32;
        if boundary > pos {
            emit(out, pos, boundary, kinds, current_owner(parse, active, marker_base));
            pos = boundary;
        }
        while ev_i < events.len() && (events[ev_i] >> 32) as u32 == boundary {
            let ev = events[ev_i];
            let is_start = ev & PHASE_START != 0;
            let idx = (ev & IDX_MASK) as u32;
            let (bit, is_marker) = if idx < marker_base {
                (parse.paints[idx as usize].kind, false)
            } else {
                (parse.markers[(idx - marker_base) as usize].kind, true)
            };
            let bi = bit.bits().trailing_zeros() as usize;
            if is_start {
                counts[bi] += 1;
                kinds |= bit;
                if is_marker {
                    active.push(idx);
                }
            } else {
                counts[bi] -= 1;
                if counts[bi] == 0 {
                    kinds &= !bit;
                }
                if is_marker
                    && let Some(p) = active.iter().position(|&a| a == idx)
                {
                    active.swap_remove(p);
                }
            }
            ev_i += 1;
        }
    }
    if pos < len {
        emit(out, pos, len, kinds, None);
    }
    debug_assert!(active.is_empty());
    debug_assert!(counts.iter().all(|&c| c == 0));
}

fn current_owner(parse: &ParseOut, active: &[u32], marker_base: u32) -> Option<(ByteRange, MarkerScope)> {
    // Markers are disjoint by construction; nested active markers would mean a
    // scan bug. Fall back to the innermost (last pushed) if it ever happens.
    debug_assert!(active.len() <= 1, "overlapping marker paints");
    active.last().map(|&idx| {
        let m = &parse.markers[(idx - marker_base) as usize];
        (m.owner, m.scope)
    })
}

fn emit(out: &mut Vec<RawSegment>, start: u32, end: u32, kinds: Kind, owner_scope: Option<(ByteRange, MarkerScope)>) {
    let kinds = if kinds.is_empty() { Kind::BODY } else { kinds };
    let (owner, scope) = match owner_scope {
        Some((o, s)) => (Some(o), Some(s)),
        None => (None, None),
    };
    // Coalesce compare includes `scope` alongside `owner` — a behavioral
    // no-op since the same owner always carries the same scope, but it
    // documents that scope is part of a raw segment's identity.
    if let Some(last) = out.last_mut()
        && last.range.end == start
        && last.kinds == kinds
        && last.owner == owner
        && last.scope == scope
    {
        last.range.end = end;
        return;
    }
    out.push(RawSegment {
        range: ByteRange::new(start, end),
        utf16: Utf16Range { start: 0, end: 0 }, // assigned by assign_utf16
        kinds,
        owner,
        scope,
    });
}

/// Fills in UTF-16 ranges with one linear pass (ASCII fast path per segment).
pub fn assign_utf16(text: &str, segments: &mut [RawSegment]) {
    let mut u16pos = 0u32;
    for seg in segments {
        let s = &text[seg.range.start as usize..seg.range.end as usize];
        let add = if s.is_ascii() {
            s.len() as u32
        } else {
            s.chars().map(|c| c.len_utf16() as u32).sum()
        };
        seg.utf16 = Utf16Range {
            start: u16pos,
            end: u16pos + add,
        };
        u16pos += add;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse, ParseOut};
    use crate::types::Extensions;

    fn flat(text: &str) -> Vec<RawSegment> {
        let mut po = ParseOut::default();
        parse(text, Extensions::all(), &mut po);
        let mut scratch = FlattenScratch::default();
        let mut out = Vec::new();
        flatten(&po, text.len() as u32, &mut scratch, &mut out);
        assign_utf16(text, &mut out);
        out
    }

    fn assert_invariants(text: &str, segs: &[RawSegment]) {
        // Sorted, non-overlapping, covering, scalar-aligned, coalesced.
        let mut pos = 0u32;
        for (i, s) in segs.iter().enumerate() {
            assert_eq!(s.range.start, pos, "gap/overlap at seg {i} in {text:?}");
            assert!(s.range.end > s.range.start, "empty seg {i}");
            assert!(text.is_char_boundary(s.range.start as usize));
            assert!(text.is_char_boundary(s.range.end as usize));
            if i > 0 {
                let prev = &segs[i - 1];
                assert!(
                    prev.kinds != s.kinds || prev.owner != s.owner,
                    "uncoalesced at {i} in {text:?}"
                );
            }
            pos = s.range.end;
        }
        assert_eq!(pos, text.len() as u32, "coverage in {text:?}");
    }

    #[test]
    fn composition_example() {
        // Composition example: `# **word**` → [MARKER "# "], [H1|MARKER "**"],
        // [H1|STRONG "word"], [H1|MARKER "**"].
        let text = "# **word**";
        let segs = flat(text);
        assert_invariants(text, &segs);
        let got: Vec<(u32, u32, Kind)> = segs.iter().map(|s| (s.range.start, s.range.end, s.kinds)).collect();
        assert_eq!(
            got,
            vec![
                (0, 2, Kind::MARKER),
                (2, 4, Kind::HEADING1 | Kind::MARKER),
                (4, 8, Kind::HEADING1 | Kind::STRONG),
                (8, 10, Kind::HEADING1 | Kind::MARKER),
            ]
        );
    }

    #[test]
    fn invariants_hold_on_corpus() {
        for text in [
            "",
            "abc",
            "# **word**\n\npara *em* and ~~st~~\n",
            "- one\n- two\n- [ ] task\n- [x] done\n",
            "> quote **bold**\n> more\n",
            "```rust\nfn x() {}\n```\n",
            "| a | b |\n|---|---|\n| **1** | 2 |\n",
            "a😀b **x** 日本語 `code`\n***nested***\n**_mixed_**\n",
            "[link](https://e.com) ![img](i.png)\n",
            "Title\n=====\n\ntext\n",
            "---\n",
            "1. one\n2. two\n",
        ] {
            let segs = flat(text);
            assert_invariants(text, &segs);
        }
    }
}
