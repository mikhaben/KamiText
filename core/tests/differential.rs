//! Differential property test: after any random edit sequence, the
//! incremental engine's state must equal a fresh `Engine::new(final_text)` —
//! segments AND elements. Catches stale-state bugs, the classic failure mode
//! of this architecture.

mod common;

use common::ATOMS;
use kamitext::{ByteRange, Engine, EngineOptions, Extensions, RevealMode};
use proptest::prelude::*;

fn snap(e: &Engine) -> (Vec<String>, Vec<String>) {
    let segs = e
        .segments_in(ByteRange::new(0, e.len_bytes()))
        .iter()
        .map(|s| {
            format!(
                "{}..{}/{}..{} {:?} {}",
                s.range.start, s.range.end, s.utf16.start, s.utf16.end, s.kinds, s.concealed
            )
        })
        .collect();
    let els = e
        .elements_in(ByteRange::new(0, e.len_bytes()))
        .iter()
        .map(|el| format!("{} {:?} {:?}", el.id, el.range, el.kind))
        .collect();
    (segs, els)
}

/// Clamps an arbitrary position to a scalar boundary at or below it.
fn align(text: &str, pos: usize) -> u32 {
    let mut p = pos.min(text.len());
    while !text.is_char_boundary(p) {
        p -= 1;
    }
    p as u32
}

#[derive(Debug, Clone)]
enum Op {
    Insert(usize, usize),        // position seed, atom index
    Delete(usize, usize),        // position seed, length seed
    Replace(usize, usize, usize),
    Select(usize, usize),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<usize>(), 0..ATOMS.len()).prop_map(|(p, a)| Op::Insert(p, a)),
        (any::<usize>(), 0..40usize).prop_map(|(p, l)| Op::Delete(p, l)),
        (any::<usize>(), 0..40usize, 0..ATOMS.len()).prop_map(|(p, l, a)| Op::Replace(p, l, a)),
        (any::<usize>(), any::<usize>()).prop_map(|(a, b)| Op::Select(a, b)),
    ]
}

fn run_differential(seed_doc: &str, ops: &[Op], options: EngineOptions) {
    let mut e = Engine::new(seed_doc, options);
    for op in ops {
        match op {
            Op::Insert(p, a) => {
                let at = align(e.text(), p % (e.text().len() + 1));
                e.apply_edit(ByteRange::new(at, at), ATOMS[*a]).unwrap();
            }
            Op::Delete(p, l) => {
                let s = align(e.text(), p % (e.text().len() + 1));
                let end = align(e.text(), (s as usize + l).min(e.text().len()));
                let (s, end) = (s.min(end), s.max(end));
                e.apply_edit(ByteRange::new(s, end), "").unwrap();
            }
            Op::Replace(p, l, a) => {
                let s = align(e.text(), p % (e.text().len() + 1));
                let end = align(e.text(), (s as usize + l).min(e.text().len()));
                let (s, end) = (s.min(end), s.max(end));
                e.apply_edit(ByteRange::new(s, end), ATOMS[*a]).unwrap();
            }
            Op::Select(a, b) => {
                let s = align(e.text(), a % (e.text().len() + 1));
                let t = align(e.text(), b % (e.text().len() + 1));
                e.set_selection(ByteRange::new(s, t)).unwrap();
            }
        }

        // A no-op selection change must produce an empty patch.
        let sel = e.selection();
        let p = e.set_selection(sel).unwrap();
        assert!(p.dirty.is_empty(), "no-op selection produced {:?}", p.dirty);
    }

    // Fresh engine over the final text, same selection, must match exactly.
    let mut fresh = Engine::new(e.text(), options);
    fresh.set_selection(e.selection()).unwrap();
    assert_eq!(snap(&e), snap(&fresh), "incremental != fresh for {:?}", e.text());
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn incremental_equals_fresh(
        seed in prop::sample::select(vec!["", "# doc\n\ntext **b** here\n", "- [ ] a\n- b\n"]),
        ops in prop::collection::vec(op_strategy(), 1..12),
    ) {
        run_differential(seed, &ops, EngineOptions::default());
    }

    #[test]
    fn incremental_equals_fresh_reader_mode(
        ops in prop::collection::vec(op_strategy(), 1..8),
    ) {
        run_differential(
            "text\n",
            &ops,
            EngineOptions { reveal: RevealMode::None, extensions: Extensions::all() },
        );
    }

    #[test]
    fn incremental_equals_fresh_block_mode(
        ops in prop::collection::vec(op_strategy(), 1..8),
    ) {
        run_differential(
            "text\n",
            &ops,
            EngineOptions { reveal: RevealMode::Block, extensions: Extensions::all() },
        );
    }

    /// Element mode's selection-aware change detection is the
    /// newest surface for staleness bugs: incremental state after random
    /// edits/selections must still match a fresh reparse.
    #[test]
    fn incremental_equals_fresh_element_mode(
        seed in prop::sample::select(vec!["", "# doc\n\ntext **b** here\n", "- [ ] a\n- b\n"]),
        ops in prop::collection::vec(op_strategy(), 1..12),
    ) {
        run_differential(
            seed,
            &ops,
            EngineOptions { reveal: RevealMode::Element, extensions: Extensions::all() },
        );
    }

    #[test]
    fn incremental_equals_fresh_no_extensions(
        ops in prop::collection::vec(op_strategy(), 1..8),
    ) {
        run_differential(
            "text\n",
            &ops,
            EngineOptions { reveal: RevealMode::Line, extensions: Extensions::empty() },
        );
    }
}

/// Patch completeness: applying old segments outside the patch
/// and new segments inside must reproduce the new covering exactly.
#[test]
fn patch_is_sufficient_for_restyle() {
    let mut e = Engine::new("# a\n**b** and *c*\nplain\n", EngineOptions::default());
    let old: Vec<_> = e.segments_in(ByteRange::new(0, e.len_bytes())).to_vec();

    let patch = e.apply_edit(ByteRange::new(4, 4), "~~x~~ ").unwrap();
    let new: Vec<_> = e.segments_in(ByteRange::new(0, e.len_bytes())).to_vec();

    // Untouched bytes style identically to before (mapped across the edit:
    // positions after the edit shift by delta = new_len - old_len = 6 - 0).
    common::assert_patch_sufficient(e.text(), &old, &new, &patch.dirty, 4, 6, 6);
}
