//! Reveal policy: resolves raw segments against the current selection into
//! the public segment list.

use crate::analysis::RawSegment;
use crate::document::Document;
use crate::types::{ByteRange, MarkerScope, RevealMode, Segment};

/// The region whose intersecting spans have their markers revealed under the
/// v0 line/block formulas. `None` means "reveal nothing" (reader mode).
///
/// `RevealMode::Element` shares the `Line` formula here: the result feeds
/// `RevealContext::line_region`, consumed only by `Block`-scoped markers
/// (block markers stay line-scoped regardless of the active
/// mode). `Inline`-scoped markers under `Element` mode bypass this function
/// entirely and resolve against the selection directly, in `resolve`.
pub fn reveal_region(
    doc: &Document,
    selection: ByteRange,
    mode: RevealMode,
    blocks: &[ByteRange],
) -> Option<ByteRange> {
    match mode {
        RevealMode::None => None,
        RevealMode::Line | RevealMode::Element => {
            // Union of all physical lines intersecting the selection —
            // canonical, direction-independent (selection is normalized).
            let first = doc.line_of(selection.start);
            let last = doc.line_of(selection.end);
            Some(ByteRange::new(
                doc.line_range(first).start,
                doc.line_range(last).end,
            ))
        }
        RevealMode::Block => {
            // Union of top-level blocks intersecting the selection; between
            // blocks (blank lines) fall back to the selection itself. Blocks
            // are document-ordered and disjoint, so the intersecting run is
            // contiguous — binary-search its edges (mirrors `intersects()`:
            // a zero-width selection at `p` touches `[a, b)` when a <= p < b).
            // Load-bearing: `out.blocks` is sorted-by-start and disjoint
            // (top-level Start + Rule ranges partition the document) — the
            // binary search is wrong for any other shape.
            debug_assert!(
                blocks.windows(2).all(|w| w[0].end <= w[1].start),
                "blocks must be sorted and disjoint"
            );
            let first = blocks.partition_point(|b| b.end <= selection.start);
            let last = if selection.is_empty() {
                blocks.partition_point(|b| b.start <= selection.start)
            } else {
                blocks.partition_point(|b| b.start < selection.end)
            };
            if first < last {
                Some(ByteRange::new(blocks[first].start, blocks[last - 1].end))
            } else {
                Some(selection)
            }
        }
    }
}

/// Inputs to conceal resolution beyond the raw segments.
/// `line_region` is `reveal_region`'s output — consumed by `Block`-scoped
/// markers, and by `Inline`-scoped markers outside `Element` mode (the
/// degradation invariant: scope is irrelevant when the mode isn't
/// `Element`). `selection` is the live selection, consumed only by
/// `Inline`-scoped markers under `Element` mode.
#[derive(Debug, Clone, Copy)]
pub struct RevealContext {
    pub line_region: Option<ByteRange>,
    pub selection: ByteRange,
    pub mode: RevealMode,
}

/// Maps raw segments to public segments, resolving conceal state and
/// re-coalescing adjacent segments equal in `(kinds, concealed)`. Class
/// dispatch: non-marker segments are never concealed;
/// `Inline`-scoped markers under `RevealMode::Element` resolve against the
/// selection via the closed-overlap activation predicate; every other
/// marker resolves against `ctx.line_region`, same as v0.
pub fn resolve(raw: &[RawSegment], ctx: &RevealContext, out: &mut Vec<Segment>) {
    out.clear();
    for rs in raw {
        let concealed = match rs.owner {
            None => false,
            Some(owner) if ctx.mode == RevealMode::Element && rs.scope == Some(MarkerScope::Inline) => {
                !activated_by_selection(owner, ctx.selection)
            }
            Some(owner) => match ctx.line_region {
                None => true,
                Some(r) => !owner.intersects(r),
            },
        };
        if let Some(last) = out.last_mut()
            && last.range.end == rs.range.start
            && last.kinds == rs.kinds
            && last.concealed == concealed
        {
            last.range.end = rs.range.end;
            last.utf16.end = rs.utf16.end;
            continue;
        }
        out.push(Segment {
            range: rs.range,
            utf16: rs.utf16,
            kinds: rs.kinds,
            concealed,
        });
    }
}

/// Closed-overlap activation predicate: owner `[o.start,
/// o.end]` is activated by selection `[s, e]` (a collapsed selection is
/// `s == e == p`, the caret) iff `s <= o.end && o.start <= e`. Both
/// boundaries inclusive — a caret immediately after a closing delimiter, or
/// a selection endpoint landing exactly on one, still activates. One
/// formula for both the collapsed-caret and wide-selection case (no `\n`
/// carve-out: `Inline`-scoped owners always end at a closing delimiter
/// byte, never a line boundary); a wide selection therefore activates every
/// element it touches, a strict superset of any interior caret's
/// activation.
fn activated_by_selection(owner: ByteRange, selection: ByteRange) -> bool {
    selection.start <= owner.end && owner.start <= selection.end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Kind, Utf16Range};
    use proptest::prelude::*;

    fn marker_seg(owner: ByteRange, scope: MarkerScope) -> RawSegment {
        RawSegment {
            range: owner,
            utf16: Utf16Range {
                start: owner.start,
                end: owner.end,
            },
            kinds: Kind::MARKER,
            owner: Some(owner),
            scope: Some(scope),
        }
    }

    #[test]
    fn activation_predicate_boundary_matrix() {
        let owner = ByteRange::new(10, 14);
        // Collapsed caret.
        assert!(activated_by_selection(owner, ByteRange::new(10, 10)), "at owner.start");
        assert!(activated_by_selection(owner, ByteRange::new(14, 14)), "at owner.end (both-inclusive)");
        assert!(activated_by_selection(owner, ByteRange::new(12, 12)), "strictly inside");
        assert!(!activated_by_selection(owner, ByteRange::new(9, 9)), "one before start");
        assert!(!activated_by_selection(owner, ByteRange::new(15, 15)), "one past end");
        // Selection endpoints landing exactly on a boundary.
        assert!(activated_by_selection(owner, ByteRange::new(14, 20)), "selection starts at owner.end");
        assert!(activated_by_selection(owner, ByteRange::new(0, 10)), "selection ends at owner.start");
        assert!(!activated_by_selection(owner, ByteRange::new(0, 9)), "selection ends before owner.start");
        assert!(!activated_by_selection(owner, ByteRange::new(15, 20)), "selection starts after owner.end");
        // Adjacent elements sharing a boundary both activate at the caret.
        let left = ByteRange::new(0, 10);
        let right = ByteRange::new(10, 20);
        let caret = ByteRange::new(10, 10);
        assert!(activated_by_selection(left, caret));
        assert!(activated_by_selection(right, caret));
        // EOF: an owner ending exactly at the document length is just
        // another `owner.end` boundary — no special case in the formula.
        let eof_owner = ByteRange::new(90, 100);
        assert!(activated_by_selection(eof_owner, ByteRange::new(100, 100)));
    }

    proptest! {
        /// Wide selection activates a superset of any interior caret's
        /// activation: if a caret
        /// `p` inside `[s, e]` activates an owner, the wide selection
        /// `[s, e]` itself must too.
        #[test]
        fn wide_selection_activation_is_superset_of_interior_caret(
            owner_start in 0u32..50,
            owner_len in 1u32..20,
            s in 0u32..50,
            width in 0u32..50,
            p_seed in 0u32..1000,
        ) {
            let owner = ByteRange::new(owner_start, owner_start + owner_len);
            let e = s + width;
            let p = s + p_seed % (width + 1);
            let caret = ByteRange::new(p, p);
            let selection = ByteRange::new(s, e);
            if activated_by_selection(owner, caret) {
                prop_assert!(activated_by_selection(owner, selection));
            }
        }

        /// Degradation invariant, applied at the exact
        /// site it governs: outside `Element` mode, `resolve` must not
        /// distinguish `Inline` from `Block` scope for the same owner.
        #[test]
        fn degradation_scope_irrelevant_outside_element_mode(
            owner_start in 0u32..20,
            owner_len in 1u32..10,
            has_region in any::<bool>(),
            region_start in 0u32..20,
            region_len in 0u32..10,
            sel_start in 0u32..20,
            sel_len in 0u32..10,
            mode in prop::sample::select(vec![RevealMode::None, RevealMode::Line, RevealMode::Block]),
        ) {
            let owner = ByteRange::new(owner_start, owner_start + owner_len);
            let line_region = has_region.then(|| ByteRange::new(region_start, region_start + region_len));
            let selection = ByteRange::new(sel_start, sel_start + sel_len);
            let ctx = RevealContext { line_region, selection, mode };

            let block = [marker_seg(owner, MarkerScope::Block)];
            let inline = [marker_seg(owner, MarkerScope::Inline)];
            let mut block_out = Vec::new();
            let mut inline_out = Vec::new();
            resolve(&block, &ctx, &mut block_out);
            resolve(&inline, &ctx, &mut inline_out);
            prop_assert_eq!(block_out[0].concealed, inline_out[0].concealed);
        }
    }
}
