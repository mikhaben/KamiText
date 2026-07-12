//! Keystroke benchmark: apply_edit(single char mid-doc) +
//! segments_in(dirty) on worst-case documents (marker density ~1/34 bytes,
//! mirroring the TextKit spike's demo doc). Gate: p50 < 3 ms at 250 KB.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use kamitext::{ByteRange, Engine, EngineOptions};
use std::hint::black_box;
use std::path::PathBuf;

/// The manifest's worst-real-corpus doc by `markerBytes` (corpora/manifest.json:
/// fpb-langs.md, markerBytes 98380 / bytes 198533 — the largest marker count
/// in the corpus). Loaded at its natural size, not scaled like the synthetic
/// template above.
fn corpus_fpb_doc() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../corpora/fpb-langs.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// One template block ≈ 330 bytes with ~10 marker tokens — one per ~33 bytes,
/// matching the spike's measured worst-case marker density.
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

fn keystroke(c: &mut Criterion) {
    let mut group = c.benchmark_group("keystroke");
    for kb in [5usize, 20, 50, 100, 250] {
        let doc = build_doc(kb * 1024);
        // Edit point: middle of the doc, snapped into a body-text line so the
        // keystroke is a realistic single-char typing event.
        let mid = {
            let target = doc.len() / 2;
            let line_start = doc[..target].rfind("Meanwhile the").unwrap_or(target);
            (line_start + 10) as u32
        };
        group.bench_with_input(BenchmarkId::new("apply_edit", kb), &kb, |b, _| {
            let mut engine = Engine::new(&doc, EngineOptions::default());
            engine.set_selection(ByteRange::new(mid, mid)).unwrap();
            let mut flip = false;
            b.iter(|| {
                // Engine-only half of the keystroke: the performance gate
                // (p50 < 3 ms @ 250 KB) applies to this number.
                let ch = if flip { "y" } else { "x" };
                flip = !flip;
                black_box(
                    engine
                        .apply_edit(ByteRange::new(mid, mid + 1), ch)
                        .expect("valid edit")
                        .dirty
                        .len(),
                )
            });
        });
        group.bench_with_input(BenchmarkId::new("apply_edit+segments", kb), &kb, |b, _| {
            let mut engine = Engine::new(&doc, EngineOptions::default());
            engine.set_selection(ByteRange::new(mid, mid)).unwrap();
            let mut flip = false;
            b.iter(|| {
                // Replace one char with another (self-correcting, doc size
                // stays constant across iterations).
                let ch = if flip { "y" } else { "x" };
                flip = !flip;
                let patch = engine
                    .apply_edit(ByteRange::new(mid, mid + 1), ch)
                    .expect("valid edit");
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

    // Worst-real-corpus variant: same alternating single-char edit pattern,
    // at the doc's natural size.
    {
        let doc = corpus_fpb_doc();
        // Edit point: middle of the doc, snapped back to the nearest ASCII
        // byte (guaranteed single-byte-wide, so `mid + 1` is also a scalar
        // boundary) — the "Meanwhile the" marker from the synthetic template
        // above won't be found in real text, so this always falls through to
        // the boundary walk.
        let mid = {
            let target = doc.len() / 2;
            let mut p = doc[..target]
                .rfind("Meanwhile the")
                .map(|i| i + 10)
                .unwrap_or(target);
            while p > 0 && (!doc.is_char_boundary(p) || doc.as_bytes()[p] >= 0x80) {
                p -= 1;
            }
            p as u32
        };
        group.bench_with_input(BenchmarkId::new("apply_edit", "corpus-fpb"), &(), |b, _| {
            let mut engine = Engine::new(&doc, EngineOptions::default());
            engine.set_selection(ByteRange::new(mid, mid)).unwrap();
            let mut flip = false;
            b.iter(|| {
                let ch = if flip { "y" } else { "x" };
                flip = !flip;
                black_box(
                    engine
                        .apply_edit(ByteRange::new(mid, mid + 1), ch)
                        .expect("valid edit")
                        .dirty
                        .len(),
                )
            });
        });
        group.bench_with_input(
            BenchmarkId::new("apply_edit+segments", "corpus-fpb"),
            &(),
            |b, _| {
                let mut engine = Engine::new(&doc, EngineOptions::default());
                engine.set_selection(ByteRange::new(mid, mid)).unwrap();
                let mut flip = false;
                b.iter(|| {
                    let ch = if flip { "y" } else { "x" };
                    flip = !flip;
                    let patch = engine
                        .apply_edit(ByteRange::new(mid, mid + 1), ch)
                        .expect("valid edit");
                    let mut touched = 0u64;
                    for r in &patch.dirty {
                        for s in engine.segments_in(*r) {
                            touched ^= s.kinds.bits() ^ u64::from(s.range.end);
                        }
                    }
                    black_box(touched)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, keystroke);
criterion_main!(benches);
