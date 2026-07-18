//! Prints total bytes, MARKER-covered bytes, and density for a markdown file
//! using the engine's segment output — the corpora/manifest.json method.
//! Usage: cargo run --example marker_density -- ../corpora/rtl-mixed-scripts.md
use kamitext::{ByteRange, Engine, EngineOptions, Kind};

fn main() {
    let path = std::env::args().nth(1).expect("usage: marker_density <file.md>");
    let text = std::fs::read_to_string(&path).expect("read file");
    let engine = Engine::new(&text, EngineOptions::default());
    let total = engine.len_bytes();
    let mut marker = 0u64;
    for seg in engine.segments_in(ByteRange::new(0, total)) {
        if seg.kinds.contains(Kind::MARKER) {
            marker += u64::from(seg.range.end - seg.range.start);
        }
    }
    println!(
        "bytes: {total}\nmarkerBytes: {marker}\nmarkerDensity: {:.4}",
        marker as f64 / f64::from(total)
    );
}
