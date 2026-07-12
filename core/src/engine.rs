//! The engine: document + parse products + conceal state + patch diffing.
//! Full-document reparse per edit (v0 strategy); arenas
//! are reused across parses.

use crate::analysis::{assign_utf16, flatten, FlattenScratch, RawSegment};
use crate::behaviors;
use crate::conceal::{resolve, reveal_region, RevealContext};
use crate::document::Document;
use crate::parse::{parse, ParseOut};
use crate::patch::{diff_after_edit, diff_same_doc};
use crate::types::{
    ByteRange, EditPlan, Element, EngineOptions, KamiError, Patch, RevealMode, Segment,
};
use core::cell::Cell;
use core::marker::PhantomData;

pub struct Engine {
    doc: Document,
    options: EngineOptions,
    /// Normalized (start <= end). A caret is an empty range.
    selection: ByteRange,
    region: Option<ByteRange>,

    parsed: ParseOut,
    scratch: FlattenScratch,
    raw: Vec<RawSegment>,
    segments: Vec<Segment>,
    prev_segments: Vec<Segment>,
    dirty: Vec<ByteRange>,

    /// Engines are Send but deliberately not Sync: callers
    /// serialize access from one thread at a time.
    _not_sync: PhantomData<Cell<()>>,
}

impl Engine {
    pub fn new(text: &str, options: EngineOptions) -> Engine {
        let mut engine = Engine {
            doc: Document::new(text),
            options,
            selection: ByteRange::new(0, 0),
            region: None,
            parsed: ParseOut::default(),
            scratch: FlattenScratch::default(),
            raw: Vec::new(),
            segments: Vec::new(),
            prev_segments: Vec::new(),
            dirty: Vec::new(),
            _not_sync: PhantomData,
        };
        engine.reparse();
        engine.region = reveal_region(
            &engine.doc,
            engine.selection,
            engine.options.reveal,
            &engine.parsed.blocks,
        );
        resolve(&engine.raw, &engine.reveal_context(), &mut engine.segments);
        engine
    }

    /// The `RevealContext` for the engine's current `region`/`selection` —
    /// built fresh at every `resolve` call site.
    fn reveal_context(&self) -> RevealContext {
        RevealContext {
            line_region: self.region,
            selection: self.selection,
            mode: self.options.reveal,
        }
    }

    fn reparse(&mut self) {
        parse(self.doc.text(), self.options.extensions, &mut self.parsed);
        flatten(
            &self.parsed,
            self.doc.len_bytes(),
            &mut self.scratch,
            &mut self.raw,
        );
        assign_utf16(self.doc.text(), &mut self.raw);
    }

    /// Applies `replacement` over `range`. Strictly validated: invalid input
    /// returns `InvalidRange` and mutates nothing.
    pub fn apply_edit(&mut self, range: ByteRange, replacement: &str) -> Result<Patch, KamiError> {
        self.doc.validate_range(range)?;

        let old_len16 = self.doc.len_utf16() as i64;
        let delta = replacement.len() as i64 - range.len() as i64;

        core::mem::swap(&mut self.prev_segments, &mut self.segments);
        self.doc.apply(range, replacement);
        self.selection = map_selection(self.selection, range, replacement.len() as u32);

        self.reparse();
        self.region = reveal_region(
            &self.doc,
            self.selection,
            self.options.reveal,
            &self.parsed.blocks,
        );
        resolve(&self.raw, &self.reveal_context(), &mut self.segments);

        let delta16 = self.doc.len_utf16() as i64 - old_len16;
        diff_after_edit(
            &self.prev_segments,
            &self.segments,
            range.start,
            range.end,
            delta,
            delta16,
            &mut self.dirty,
        );
        Ok(Patch {
            dirty: self.dirty.clone(),
        })
    }

    /// Moves the selection (caret = empty range). Anchor/focus order is
    /// irrelevant: a reversed range is normalized, then validated.
    pub fn set_selection(&mut self, selection: ByteRange) -> Result<Patch, KamiError> {
        let normalized = ByteRange::new(
            selection.start.min(selection.end),
            selection.start.max(selection.end),
        );
        self.doc.validate_range(normalized)?;

        let region = reveal_region(
            &self.doc,
            normalized,
            self.options.reveal,
            &self.parsed.blocks,
        );
        // Element mode's activation predicate reads the selection directly,
        // not just the (coarser) line region — an unchanged region no
        // longer implies an unchanged reveal outcome (the case this fixes is
        // a caret moving within one line into `**bold**`, where the line
        // region never changes). The short-circuit keys on the selection
        // itself in that mode; other modes keep the region-only check.
        let unchanged = if self.options.reveal == RevealMode::Element {
            normalized == self.selection
        } else {
            region == self.region
        };
        self.selection = normalized;
        if unchanged {
            return Ok(Patch { dirty: Vec::new() });
        }
        self.region = region;

        core::mem::swap(&mut self.prev_segments, &mut self.segments);
        resolve(&self.raw, &self.reveal_context(), &mut self.segments);
        diff_same_doc(&self.prev_segments, &self.segments, &mut self.dirty);
        Ok(Patch {
            dirty: self.dirty.clone(),
        })
    }

    /// All segments intersecting `range`, as a contiguous sub-slice of the
    /// full covering (never splits segments; out-of-bounds queries clamp).
    pub fn segments_in(&self, range: ByteRange) -> &[Segment] {
        let len = self.doc.len_bytes();
        let qs = range.start.min(range.end).min(len);
        let qe = range.start.max(range.end).min(len);
        let i = self
            .segments
            .partition_point(|s| s.range.end <= qs);
        if qs == qe {
            if i < self.segments.len() && self.segments[i].range.start <= qs {
                return &self.segments[i..i + 1];
            }
            return &self.segments[i..i];
        }
        let j = self.segments.partition_point(|s| s.range.start < qe);
        &self.segments[i..j]
    }

    /// All elements intersecting `range`, as a contiguous sub-slice of the
    /// document-ordered element list. The slice is a conservative superset
    /// bounded by "first element not provably left of the query" and "last
    /// element starting before the query end"; adapters filter by range.
    pub fn elements_in(&self, range: ByteRange) -> &[Element] {
        let len = self.doc.len_bytes();
        let qs = range.start.min(range.end).min(len);
        let qe = range.start.max(range.end).min(len);
        let els = &self.parsed.elements;
        // First candidate: prefix-max of ends is monotone, so this binary
        // search finds the first index whose element (or an earlier one) can
        // still reach past qs. Everything before it ends at or before qs.
        let first = self.parsed.elements_max_end.partition_point(|&me| me <= qs);
        // Last candidate: starts are document-ordered. Empty query = caret
        // containment (closed at the start edge), matching intersects().
        let last = if qs == qe {
            els.partition_point(|e| e.range.start <= qs)
        } else {
            els.partition_point(|e| e.range.start < qe)
        };
        &els[first..last]
    }

    pub fn text(&self) -> &str {
        self.doc.text()
    }

    pub fn len_bytes(&self) -> u32 {
        self.doc.len_bytes()
    }

    pub fn len_utf16(&self) -> u32 {
        self.doc.len_utf16()
    }

    pub fn selection(&self) -> ByteRange {
        self.selection
    }

    pub fn options(&self) -> EngineOptions {
        self.options
    }

    pub fn byte_to_utf16(&self, offset: u32) -> u32 {
        self.doc.byte_to_utf16(offset)
    }

    /// Rounds down to a scalar start. Queries stay lenient;
    /// validation strictness applies to mutations only.
    pub fn utf16_to_byte(&self, offset: u32) -> u32 {
        self.doc.utf16_to_byte(offset)
    }

    /// List/task/quote continuation or exit-on-empty. `at` is a
    /// collapsed caret offset; misaligned input is an error, surfaced via
    /// the `Result` return.
    pub fn newline_plan(&self, at: u32) -> Result<Option<EditPlan>, KamiError> {
        self.doc.validate_offset(at)?;
        Ok(behaviors::newline_plan(
            &self.doc,
            &self.parsed.verbatim_blocks,
            self.options
                .extensions
                .contains(crate::types::Extensions::TASK_LISTS),
            at,
        ))
    }

    /// Flips the task checkbox whose item contains `at`.
    pub fn toggle_task_plan(&self, at: u32) -> Result<Option<EditPlan>, KamiError> {
        self.doc.validate_offset(at)?;
        Ok(behaviors::toggle_task_plan(
            &self.parsed.task_boxes,
            self.doc.len_bytes(),
            at,
        ))
    }
}

/// Maps a selection endpoint across an edit: positions before the edit stay,
/// positions after shift by the length delta, positions inside collapse to
/// the end of the replacement (typing UX: the caret lands after the insert).
fn map_offset(p: u32, edit: ByteRange, repl_len: u32) -> u32 {
    if p < edit.start {
        p
    } else if p >= edit.end {
        p - edit.len() + repl_len
    } else {
        edit.start + repl_len
    }
}

fn map_selection(sel: ByteRange, edit: ByteRange, repl_len: u32) -> ByteRange {
    let s = map_offset(sel.start, edit, repl_len);
    let e = map_offset(sel.end, edit, repl_len);
    ByteRange::new(s.min(e), s.max(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ElementKind;
    use proptest::prelude::*;

    /// Building blocks that exercise every element-bearing construct when
    /// concatenated, including a task nested inside a quote nested inside a
    /// list (the case the prefix-max bound exists to handle correctly).
    const ATOMS: &[&str] = &[
        "# h\n", "**b** ", "[l](u) ", "![i](s) ", "```\ncode\n```\n",
        "- [ ] t\n", "- [x] t\n", "- outer\n  > - [ ] inner\n", "> q\n",
        "- li\n", "plain ", "\n",
    ];

    fn doc_strategy() -> impl Strategy<Value = String> {
        prop::collection::vec(prop::sample::select(ATOMS), 1..8).prop_map(|v| v.concat())
    }

    /// Query ranges biased toward 0, EOF and real element boundaries (both as
    /// carets and as range endpoints), plus arbitrary noise — the cases most
    /// likely to expose an off-by-one in the binary-search bound.
    fn query_strategy(doc: &str) -> impl Strategy<Value = ByteRange> + use<> {
        let e = Engine::new(doc, EngineOptions::default());
        let len = e.len_bytes();
        let mut boundaries: Vec<u32> = vec![0, len];
        for el in &e.parsed.elements {
            boundaries.push(el.range.start);
            boundaries.push(el.range.end);
        }
        prop_oneof![
            Just(ByteRange::new(0, 0)),
            Just(ByteRange::new(len, len)),
            prop::sample::select(boundaries).prop_map(|p| ByteRange::new(p, p)),
            (0..=len, 0..=len).prop_map(|(a, b)| ByteRange::new(a.min(b), a.max(b))),
        ]
    }

    proptest! {
        /// `elements_in(q)` filtered by intersection must set-equal a
        /// brute-force scan of the full element list filtered by
        /// intersection — the raw (unfiltered) slices may legitimately
        /// differ, since both are conservative supersets.
        #[test]
        fn elements_in_matches_brute_force_intersection(
            (doc, range) in doc_strategy().prop_flat_map(|doc| {
                let q = query_strategy(&doc);
                (Just(doc), q)
            })
        ) {
            let e = Engine::new(&doc, EngineOptions::default());
            let len = e.len_bytes();
            let qs = range.start.min(range.end).min(len);
            let qe = range.start.max(range.end).min(len);
            let q = ByteRange::new(qs, qe);

            let brute: Vec<Element> = e
                .parsed
                .elements
                .iter()
                .copied()
                .filter(|el| el.range.intersects(q))
                .collect();
            let got: Vec<Element> = e
                .elements_in(range)
                .iter()
                .copied()
                .filter(|el| el.range.intersects(q))
                .collect();
            prop_assert_eq!(got, brute);
        }
    }

    #[test]
    fn caret_at_element_start_is_included() {
        let e = Engine::new("[l](u)\n", EngineOptions::default());
        let all = e.elements_in(ByteRange::new(0, e.len_bytes()));
        assert_eq!(all.len(), 1);
        let start = all[0].range.start;
        assert_eq!(start, 0);
        let got = e.elements_in(ByteRange::new(start, start));
        assert!(got.iter().any(|el| el.id == all[0].id), "caret at start must hit the element");
    }

    #[test]
    fn caret_at_element_end_is_excluded() {
        let e = Engine::new("[l](u)\n", EngineOptions::default());
        let all = e.elements_in(ByteRange::new(0, e.len_bytes()));
        assert_eq!(all.len(), 1);
        let end = all[0].range.end;
        let got = e.elements_in(ByteRange::new(end, end));
        assert!(
            got.iter().filter(|el| el.range.intersects(ByteRange::new(end, end))).count() == 0,
            "caret at end must not hit the element (half-open)"
        );
    }

    #[test]
    fn query_strictly_between_siblings_hits_neither() {
        let text = "[a](u) mid [b](v)\n";
        let e = Engine::new(text, EngineOptions::default());
        let all = e.elements_in(ByteRange::new(0, e.len_bytes()));
        assert_eq!(all.len(), 2);
        let gap_start = all[0].range.end;
        let gap_end = all[1].range.start;
        assert!(gap_start < gap_end, "test doc must have a real gap between siblings");
        let mid = ByteRange::new(gap_start, gap_end);
        let got = e.elements_in(mid);
        assert!(
            got.iter().filter(|el| el.range.intersects(mid)).count() == 0,
            "query strictly between siblings must not intersect either"
        );
    }

    #[test]
    fn nested_task_in_quote_in_list_caret_hits_innermost() {
        let text = "- outer\n  > - [ ] inner\n";
        let e = Engine::new(text, EngineOptions::default());
        let all = e.elements_in(ByteRange::new(0, e.len_bytes()));
        let task = all
            .iter()
            .find(|el| matches!(el.kind, ElementKind::Task { .. }))
            .expect("nested task element");
        let caret = task.range.start + task.range.len() / 2;
        let got = e.elements_in(ByteRange::new(caret, caret));
        assert!(
            got.iter().any(|el| el.id == task.id),
            "caret inside the innermost task must hit it"
        );
    }
}
