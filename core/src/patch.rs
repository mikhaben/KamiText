//! Old-vs-new segment diff → coalesced dirty ranges.
//!
//! Patches are conservative: they may over-report, never under-report. Dirty
//! ranges are expanded to whole affected segments in both the old and new
//! lists, so boundary re-coalescing can never leak outside a reported range.

use crate::types::{ByteRange, Segment};

/// Diff after a text edit `[start, old_end) -> replacement` with byte delta
/// `delta` and UTF-16 delta `delta16`. Both lists are sorted and covering;
/// old is in pre-edit coordinates, new in post-edit coordinates.
///
/// Prefix/suffix trim: identical segments entirely before the edit and
/// identical-modulo-delta segments entirely after it are provably unchanged;
/// everything between is one dirty range (in new coordinates).
pub fn diff_after_edit(
    old: &[Segment],
    new: &[Segment],
    edit_start: u32,
    old_edit_end: u32,
    delta: i64,
    delta16: i64,
    out: &mut Vec<ByteRange>,
) {
    out.clear();

    let mut prefix = 0usize;
    while prefix < old.len() && prefix < new.len() {
        let (o, n) = (&old[prefix], &new[prefix]);
        if o == n && o.range.end <= edit_start {
            prefix += 1;
        } else {
            break;
        }
    }

    let mut suffix = 0usize;
    while suffix < old.len() - prefix && suffix < new.len() - prefix {
        let o = &old[old.len() - 1 - suffix];
        let n = &new[new.len() - 1 - suffix];
        let shifted = o.range.start >= old_edit_end
            && (o.range.start as i64 + delta) as u32 == n.range.start
            && (o.range.end as i64 + delta) as u32 == n.range.end
            && (o.utf16.start as i64 + delta16) as u32 == n.utf16.start
            && (o.utf16.end as i64 + delta16) as u32 == n.utf16.end;
        if shifted && o.kinds == n.kinds && o.concealed == n.concealed {
            suffix += 1;
        } else {
            break;
        }
    }

    if prefix < new.len() - suffix {
        out.push(ByteRange::new(
            new[prefix].range.start,
            new[new.len() - 1 - suffix].range.end,
        ));
    }
}

/// Diff over an unchanged document (selection change): walks both coverings,
/// finds intervals where `(kinds, concealed)` differ, expands each to whole
/// old/new segment boundaries and coalesces.
pub fn diff_same_doc(old: &[Segment], new: &[Segment], out: &mut Vec<ByteRange>) {
    out.clear();
    debug_assert_eq!(
        old.last().map(|s| s.range.end),
        new.last().map(|s| s.range.end)
    );

    let (mut i, mut j) = (0usize, 0usize);
    let mut pos = 0u32;
    let mut open: Option<ByteRange> = None;

    while i < old.len() && j < new.len() {
        let (o, n) = (&old[i], &new[j]);
        let end = o.range.end.min(n.range.end);
        if o.kinds != n.kinds || o.concealed != n.concealed {
            // Expand to whole containing segments on both sides.
            let d_start = o.range.start.min(n.range.start).min(pos);
            let d_end = o.range.end.max(n.range.end);
            open = Some(match open {
                Some(cur) if d_start <= cur.end => {
                    ByteRange::new(cur.start, cur.end.max(d_end))
                }
                Some(cur) => {
                    out.push(cur);
                    ByteRange::new(d_start, d_end)
                }
                None => ByteRange::new(d_start, d_end),
            });
        }
        pos = end;
        if o.range.end == end {
            i += 1;
        }
        if n.range.end == end {
            j += 1;
        }
    }
    if let Some(cur) = open {
        out.push(cur);
    }

    debug_assert!(out.windows(2).all(|w| w[0].end < w[1].start));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Kind, Segment, Utf16Range};

    fn seg(start: u32, end: u32, kinds: Kind, concealed: bool) -> Segment {
        Segment {
            range: ByteRange::new(start, end),
            utf16: Utf16Range { start, end },
            kinds,
            concealed,
        }
    }

    #[test]
    fn same_doc_no_change_is_empty() {
        let a = [seg(0, 5, Kind::BODY, false), seg(5, 8, Kind::MARKER, true)];
        let mut out = Vec::new();
        diff_same_doc(&a, &a, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn same_doc_conceal_flip_expands_to_segments() {
        let old = [seg(0, 5, Kind::BODY, false), seg(5, 8, Kind::MARKER, true)];
        let new = [seg(0, 5, Kind::BODY, false), seg(5, 8, Kind::MARKER, false)];
        let mut out = Vec::new();
        diff_same_doc(&old, &new, &mut out);
        assert_eq!(out, vec![ByteRange::new(5, 8)]);
    }

    #[test]
    fn same_doc_boundary_shift_covers_both() {
        // Coalescing differences: old has one seg, new splits it.
        let old = [seg(0, 8, Kind::MARKER, true)];
        let new = [seg(0, 4, Kind::MARKER, true), seg(4, 8, Kind::MARKER, false)];
        let mut out = Vec::new();
        diff_same_doc(&old, &new, &mut out);
        assert_eq!(out, vec![ByteRange::new(0, 8)]);
    }

    #[test]
    fn edit_diff_trims_prefix_suffix() {
        // "aaXbb" -> "aaYYbb": middle segment changes, delta +1.
        let old = [
            seg(0, 2, Kind::BODY, false),
            seg(2, 3, Kind::STRONG, false),
            seg(3, 5, Kind::BODY, false),
        ];
        let new = [
            seg(0, 2, Kind::BODY, false),
            seg(2, 4, Kind::EMPHASIS, false),
            Segment {
                range: ByteRange::new(4, 6),
                utf16: Utf16Range { start: 4, end: 6 },
                kinds: Kind::BODY,
                concealed: false,
            },
        ];
        let mut out = Vec::new();
        diff_after_edit(&old, &new, 2, 3, 1, 1, &mut out);
        assert_eq!(out, vec![ByteRange::new(2, 4)]);
    }
}
