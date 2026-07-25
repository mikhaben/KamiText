//! Keystroke timing on the REAL corpus (release), and a hammer loop for a
//! sampling profiler.
//!
//! `profile.rs` measures the synthetic template, whose marker density (~1/33
//! bytes) is an order of magnitude below what real link-heavy markdown reaches
//! (`fpb-langs.md`: 0.50, `public-apis.md`: 0.31). Keystroke cost scales with
//! the number of paint/marker events, not with document bytes, so the synthetic
//! doc systematically under-reports the worst real case.
//!
//! - `cargo run --release --example profile_corpus` — p50/p99 per corpus doc.
//! - `cargo run --release --example profile_corpus -- <file> <seconds>` — hammer
//!   one document in a loop, for `sample`/`samply` to attribute by function.

use kamitext::{ByteRange, Engine, EngineOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../corpora")
}

fn corpus_docs() -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(corpus_dir()) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(p).ok()?;
            Some((p.file_name()?.to_string_lossy().into_owned(), text))
        })
        .collect()
}

/// A scalar-aligned single-byte ASCII edit point mid-document, so `mid..mid+1`
/// always replaces exactly one char with one char (document length constant
/// across iterations).
fn edit_point(doc: &str) -> Option<u32> {
    let mut p = doc.len() / 2;
    while p > 0
        && (!doc.is_char_boundary(p) || doc.as_bytes()[p] >= 0x80 || doc.as_bytes()[p] == b'\n')
    {
        p -= 1;
    }
    (p > 0).then_some(p as u32)
}

fn keystrokes(doc: &str, iters: usize) -> Option<(f64, f64)> {
    let mid = edit_point(doc)?;
    let mut engine = Engine::new(doc, EngineOptions::default());
    engine.set_selection(ByteRange::new(mid, mid)).ok()?;
    let mut samples = Vec::with_capacity(iters);
    for i in 0..iters {
        let ch = if i % 2 == 0 { "x" } else { "y" };
        let start = Instant::now();
        let patch = engine.apply_edit(ByteRange::new(mid, mid + 1), ch).ok()?;
        std::hint::black_box(&patch);
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).expect("no NaN timings"));
    Some((samples[iters / 2], samples[iters * 99 / 100]))
}

/// Hammer one document until the deadline — the shape a sampling profiler wants.
fn hammer(path: &Path, seconds: f64) {
    let doc = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let mid = edit_point(&doc).expect("document has an interior ASCII edit point");
    let mut engine = Engine::new(&doc, EngineOptions::default());
    engine.set_selection(ByteRange::new(mid, mid)).unwrap();
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    let mut n = 0u64;
    while Instant::now() < deadline {
        for i in 0..32 {
            let ch = if i % 2 == 0 { "x" } else { "y" };
            std::hint::black_box(engine.apply_edit(ByteRange::new(mid, mid + 1), ch).unwrap());
            n += 1;
        }
    }
    println!("{} keystrokes over {seconds}s on {path:?}", n);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [file, seconds] = args.as_slice() {
        let path = corpus_dir().join(file);
        hammer(&path, seconds.parse().unwrap_or(5.0));
        return;
    }

    println!("{:<40} {:>8} {:>10} {:>10}", "document", "KB", "p50 ms", "p99 ms");
    for (name, doc) in corpus_docs() {
        // Small documents need more iterations to make the p99 meaningful;
        // large ones would take minutes at that count.
        let iters = if doc.len() > 100 * 1024 { 60 } else { 400 };
        let Some((p50, p99)) = keystrokes(&doc, iters) else { continue };
        println!(
            "{:<40} {:>8.0} {:>10.3} {:>10.3}",
            name,
            doc.len() as f64 / 1024.0,
            p50,
            p99
        );
    }
}
