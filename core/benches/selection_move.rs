//! `selection_move` benchmark: `set_selection`
//! cost across three trajectories (within-line into/out of an element,
//! cross-line, no-op) in both `Line` and `Element` modes. Gate: Element-mode
//! moves <= the existing `apply_edit+segments` numbers (keystroke.rs) at the
//! same document size, and absolute < 1 ms at 250 KB.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use kamitext::{ByteRange, Engine, EngineOptions, RevealMode};
use std::hint::black_box;

/// Mirrors keystroke.rs's document shape (marker density ~1/34 bytes) so the
/// gate comparison is apples-to-apples.
const TEMPLATE: &str = "\
## Quarterly report summary for the finance team\n\
The projections stayed roughly flat against last quarter.\n\
- [ ] follow up on the **budget** review item\n\
Meanwhile the engineering group closed out remaining work.\n\
> planning note with `inline code` for reference\n\
See [the docs](https://example.com/guide) or *ask* anyone.\n\n";

fn build_doc(target_bytes: usize) -> String {
    let mut s = String::with_capacity(target_bytes + TEMPLATE.len());
    while s.len() < target_bytes {
        s.push_str(TEMPLATE);
    }
    s
}

fn trajectory(c: &mut Criterion, name: &str, positions: fn(&str) -> (u32, u32)) {
    let mut group = c.benchmark_group(name);
    for kb in [5usize, 20, 50, 100, 250] {
        let doc = build_doc(kb * 1024);
        let (a, b) = positions(&doc);
        for mode in [RevealMode::Line, RevealMode::Element] {
            let label = match mode {
                RevealMode::Line => "line",
                RevealMode::Element => "element",
                _ => unreachable!(),
            };
            group.bench_with_input(BenchmarkId::new(label, kb), &kb, |bch, _| {
                let mut engine = Engine::new(
                    &doc,
                    EngineOptions {
                        reveal: mode,
                        ..Default::default()
                    },
                );
                engine.set_selection(ByteRange::new(a, a)).unwrap();
                let mut flip = false;
                bch.iter(|| {
                    let p = if flip { a } else { b };
                    flip = !flip;
                    let patch = engine.set_selection(ByteRange::new(p, p)).unwrap();
                    let mut touched = 0u64;
                    for r in &patch.dirty {
                        for s in engine.segments_in(*r) {
                            touched ^= s.kinds.bits() ^ u64::from(s.range.end);
                        }
                    }
                    black_box(touched)
                });
            });
        }
    }
    group.finish();
}

fn within_line_into_out_of_element(c: &mut Criterion) {
    trajectory(c, "selection_move/within_line", |doc| {
        // "budget" sits inside a **strong** span on the task line; "follow"
        // a few bytes earlier on the same line is outside any owner.
        let inside = doc.find("**budget**").unwrap() as u32 + 4;
        let outside = doc.find("follow up").unwrap() as u32 + 2;
        (outside, inside)
    });
}

fn cross_line(c: &mut Criterion) {
    trajectory(c, "selection_move/cross_line", |doc| {
        let a = doc.find("Quarterly").unwrap() as u32 + 2;
        let b = doc.find("Meanwhile").unwrap() as u32 + 2;
        (a, b)
    });
}

fn no_op(c: &mut Criterion) {
    trajectory(c, "selection_move/no_op", |doc| {
        let a = doc.find("Meanwhile").unwrap() as u32 + 2;
        (a, a)
    });
}

criterion_group!(benches, within_line_into_out_of_element, cross_line, no_op);
criterion_main!(benches);
