//! Rough phase timing at 250 KB (release): where does the keystroke go?

use kamitext::{ByteRange, Engine, EngineOptions};
use std::time::Instant;

const TEMPLATE: &str = "\
## Quarterly report summary for the finance team\n\
The projections stayed roughly flat against last quarter.\n\
- [ ] follow up on the **budget** review item\n\
Meanwhile the engineering group closed out remaining work.\n\
> planning note with `inline code` for reference\n\
See [the docs](https://example.com/guide) or *ask* anyone.\n\n";

fn main() {
    let mut doc = String::new();
    while doc.len() < 250 * 1024 {
        doc.push_str(TEMPLATE);
    }

    // Raw pulldown parse cost for reference.
    let t = Instant::now();
    let mut n = 0usize;
    for _ in 0..20 {
        use pulldown_cmark::{Options, Parser};
        let mut o = Options::empty();
        o.insert(Options::ENABLE_TABLES);
        o.insert(Options::ENABLE_TASKLISTS);
        o.insert(Options::ENABLE_STRIKETHROUGH);
        n += Parser::new_ext(&doc, o).into_offset_iter().count();
    }
    println!("pulldown parse only: {:?}/iter ({} events)", t.elapsed() / 20, n / 20);

    let t = Instant::now();
    let mut e = Engine::new(&doc, EngineOptions::default());
    println!("Engine::new: {:?}", t.elapsed());

    let mid = (doc.len() / 2) as u32;
    let mut m = mid;
    while !doc.is_char_boundary(m as usize) {
        m -= 1;
    }
    e.set_selection(ByteRange::new(m, m)).unwrap();

    // Percentile sampler: criterion reports medians only, but the kami-core
    // §7 gate names p50 AND p99 — raw sorted samples are the only way to
    // read the tail.
    let iters = 2000;
    let mut samples = Vec::with_capacity(iters);
    for i in 0..iters {
        let ch = if i % 2 == 0 { "x" } else { "y" };
        let t = Instant::now();
        let p = e.apply_edit(ByteRange::new(m, m + 1), ch).unwrap();
        std::hint::black_box(&p);
        samples.push(t.elapsed());
    }
    samples.sort_unstable();
    println!(
        "apply_edit @250KB over {iters} keystrokes: p50 {:?}  p99 {:?}",
        samples[iters / 2],
        samples[iters * 99 / 100],
    );
}
