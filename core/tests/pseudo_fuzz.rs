//! Deterministic pseudo-fuzz: ≥100k arbitrary apply_edit /
//! set_selection calls — including invalid ranges, scalar-splitting offsets
//! and arbitrary (possibly multi-byte) replacements. The engine must never
//! panic: every call either succeeds or returns an error leaving state
//! untouched. Periodically cross-checked against a fresh parse.

use kamitext::{ByteRange, Engine, EngineOptions, RevealMode};

/// xorshift64* — deterministic, dependency-free PRNG.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const REPLACEMENTS: &[&str] = &[
    "", "a", "😀", "日", "\u{200D}", "**", "*", "`", "```", "#", "# ", "\n",
    "\n\n", "- ", "- [ ] ", "> ", "|", "---", "~~", "[", "](", ")", "===",
    "***bold***", "`code`", "\t", "  ", "\u{FE0F}", "\r", "\r\n",
];

#[test]
fn pseudo_fuzz_100k_no_panic_error_or_consistent() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut e = Engine::new("# seed\n\n- [ ] task **b**\n", EngineOptions::default());
    let mut shadow = String::from(e.text());
    let mut applied = 0u32;
    let mut rejected = 0u32;

    for i in 0..100_000u32 {
        let len = e.len_bytes() as u64;
        // Half raw (offsets beyond len, reversed, mid-scalar — must be
        // rejected), half aligned (must be applied) so state evolves deeply.
        let (a, b) = if rng.below(2) == 0 {
            (rng.below(len + 8) as u32, rng.below(len + 8) as u32)
        } else {
            let mut x = rng.below(len + 1) as u32;
            let mut y = rng.below(len + 1) as u32;
            while !e.text().is_char_boundary(x as usize) {
                x -= 1;
            }
            while !e.text().is_char_boundary(y as usize) {
                y -= 1;
            }
            (x.min(y), x.max(y))
        };

        match rng.below(10) {
            0..=6 => {
                let repl = REPLACEMENTS[rng.below(REPLACEMENTS.len() as u64) as usize];
                let range = ByteRange::new(a, b);
                match e.apply_edit(range, repl) {
                    Ok(patch) => {
                        applied += 1;
                        // Mirror the edit; text must match exactly.
                        shadow.replace_range(a as usize..b as usize, repl);
                        assert_eq!(e.text(), shadow, "text desync at iter {i}");
                        for w in patch.dirty.windows(2) {
                            assert!(w[0].end < w[1].start, "bad patch at iter {i}");
                        }
                        for r in &patch.dirty {
                            assert!(r.start <= r.end && r.end <= e.len_bytes());
                        }
                    }
                    Err(_) => {
                        rejected += 1;
                        // Error ⇒ no mutation.
                        assert_eq!(e.text(), shadow, "mutated on error at iter {i}");
                        // The rejected range must indeed have been invalid.
                        let valid = a <= b
                            && (b as usize) <= shadow.len()
                            && shadow.is_char_boundary(a as usize)
                            && shadow.is_char_boundary(b as usize);
                        assert!(!valid, "valid range rejected at iter {i}: {a}..{b}");
                    }
                }
            }
            7..=8 => {
                let _ = e.set_selection(ByteRange::new(a, b));
                assert_eq!(e.text(), shadow);
            }
            _ => {
                // Behavior queries must never panic either.
                let _ = e.newline_plan(a);
                let _ = e.toggle_task_plan(a);
                let _ = e.byte_to_utf16(a);
                let _ = e.utf16_to_byte(a);
                let _ = e.segments_in(ByteRange::new(a, b));
                let _ = e.elements_in(ByteRange::new(a, b));
            }
        }

        // Docs must not grow unboundedly (keeps the run fast): trim.
        if e.len_bytes() > 4096 {
            let cut = e.len_bytes() / 2;
            let mut c = cut;
            while !e.text().is_char_boundary(c as usize) {
                c -= 1;
            }
            e.apply_edit(ByteRange::new(0, c), "").unwrap();
            shadow.replace_range(0..c as usize, "");
        }

        // Periodic consistency audit against a fresh engine.
        if i % 4096 == 0 {
            let mut fresh = Engine::new(e.text(), EngineOptions::default());
            fresh.set_selection(e.selection()).unwrap();
            assert_eq!(
                e.segments_in(ByteRange::new(0, e.len_bytes())),
                fresh.segments_in(ByteRange::new(0, fresh.len_bytes())),
                "stale state at iter {i}"
            );
            assert_eq!(e.len_utf16(), fresh.len_utf16());
        }
    }

    // Sanity: the fuzz actually exercised both paths.
    assert!(applied > 10_000, "only {applied} edits applied");
    assert!(rejected > 1_000, "only {rejected} edits rejected");
}

#[test]
fn pseudo_fuzz_reader_mode_20k() {
    let mut rng = Rng(0xDEADBEEFCAFEF00D);
    let mut e = Engine::new(
        "```\nx\n```\n> q\n",
        EngineOptions {
            reveal: RevealMode::None,
            ..Default::default()
        },
    );
    for _ in 0..20_000u32 {
        let len = e.len_bytes() as u64;
        let a = rng.below(len + 4) as u32;
        let b = rng.below(len + 4) as u32;
        let repl = REPLACEMENTS[rng.below(REPLACEMENTS.len() as u64) as usize];
        let _ = e.apply_edit(ByteRange::new(a, b), repl);
        if e.len_bytes() > 2048 {
            let mut c = e.len_bytes() / 2;
            while !e.text().is_char_boundary(c as usize) {
                c -= 1;
            }
            e.apply_edit(ByteRange::new(0, c), "").unwrap();
        }
    }
}
