//! Real-markdown corpus sweep. Every
//! `corpora/*.md` document (never `manifest.json` — that file is
//! documentation for humans and the licensing record) gets: a full-parse
//! invariant sweep, a seeded deterministic edit/selection script checked
//! incremental-vs-fresh after every op under both `Line` and `Element`
//! reveal modes, and one patch-sufficiency spot check.
//!
//! Script length scales inversely with document size so the whole sweep
//! stays well under the 60 s `cargo test` budget; `KAMI_CORPUS_FULL=1`
//! multiplies script length by 8 for a slower, more thorough run.

mod common;

use kamitext::{ByteRange, Engine, EngineOptions, RevealMode};
use std::path::{Path, PathBuf};

fn corpus_dir() -> PathBuf {
    // core/ is the crate root; corpora/ lives at the repo root next to it
    // (same pattern as export-fixtures.rs's fixtures/ join).
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../corpora")
}

fn corpus_files() -> Vec<PathBuf> {
    let dir = corpus_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read corpora dir {dir:?}: {e}"))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
        .collect();
    files.sort();
    files
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// Deterministic per-file seed (no RNG from time): sum of the file name's
/// bytes, mixed and forced odd so the xorshift generator never locks at 0.
fn seed_for(path: &Path) -> u64 {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("corpus");
    let sum: u64 = name.bytes().map(u64::from).sum();
    sum.wrapping_mul(0x9E3779B97F4A7C15) | 1
}

/// Script length for a document of `bytes` size: the per-op cost is a
/// fresh-engine full reparse + whole-document snapshot, so larger docs get
/// shorter scripts. `KAMI_CORPUS_FULL=1` unlocks full-length scripts.
fn op_count(bytes: usize) -> usize {
    let base = if bytes > 150_000 {
        10
    } else if bytes > 50_000 {
        25
    } else {
        60
    };
    let full = std::env::var("KAMI_CORPUS_FULL").as_deref() == Ok("1");
    if full { base * 8 } else { base }
}

/// Builds a script of `n` valid ops against `text`, advancing a scratch copy
/// of the text exactly as the ops would mutate it. The resulting script is
/// mode-independent (text content never depends on `RevealMode`), so the
/// same script replays identically against both a `Line`-mode and an
/// `Element`-mode engine.
fn build_script(seed: u64, text: &str, n: usize) -> Vec<common::Op> {
    let mut rng = common::FuzzRng(seed);
    let mut scratch = text.to_string();
    let mut ops = Vec::with_capacity(n);
    for _ in 0..n {
        let op = common::next_valid_op(&mut rng, &scratch);
        match &op {
            common::Op::Insert { at, s } => scratch.insert_str(*at as usize, s),
            common::Op::Delete { range } => {
                scratch.replace_range(range.start as usize..range.end as usize, "");
            }
            common::Op::Replace { range, s } => {
                scratch.replace_range(range.start as usize..range.end as usize, s);
            }
            common::Op::Select { .. } => {}
        }
        ops.push(op);
    }
    ops
}

/// Replays `ops` against a fresh engine in `mode`, comparing the incremental
/// engine's full segment + element snapshot to a from-scratch oracle
/// (`Engine::new` over the current text, selection replayed) after every op.
fn run_script_checked(path: &Path, text: &str, ops: &[common::Op], mode: RevealMode) {
    let options = EngineOptions { reveal: mode, ..EngineOptions::default() };
    let mut e = Engine::new(text, options);
    for (i, op) in ops.iter().enumerate() {
        match op {
            common::Op::Insert { at, s } => {
                e.apply_edit(ByteRange::new(*at, *at), s).unwrap();
            }
            common::Op::Delete { range } => {
                e.apply_edit(*range, "").unwrap();
            }
            common::Op::Replace { range, s } => {
                e.apply_edit(*range, s).unwrap();
            }
            common::Op::Select { range } => {
                e.set_selection(*range).unwrap();
            }
        }

        let mut fresh = Engine::new(e.text(), options);
        fresh.set_selection(e.selection()).unwrap();
        assert_eq!(
            e.segments_in(ByteRange::new(0, e.len_bytes())),
            fresh.segments_in(ByteRange::new(0, fresh.len_bytes())),
            "segment desync in {path:?} ({mode:?}) at op {i}: {op:?}"
        );
        assert_eq!(
            e.elements_in(ByteRange::new(0, e.len_bytes())),
            fresh.elements_in(ByteRange::new(0, fresh.len_bytes())),
            "element desync in {path:?} ({mode:?}) at op {i}: {op:?}"
        );
    }
}

/// (a) Full-parse invariant sweep under default options.
#[test]
fn corpus_full_parse_invariants() {
    for path in corpus_files() {
        let text = read(&path);
        let e = Engine::new(&text, EngineOptions::default());
        common::assert_invariants(&e);
    }
}

/// (b) Seeded deterministic edit/selection script: incremental ≡ fresh after
/// every op, under both `Line` and `Element` reveal modes.
#[test]
fn corpus_incremental_equals_fresh_both_modes() {
    for path in corpus_files() {
        let text = read(&path);
        let n = op_count(text.len());
        let ops = build_script(seed_for(&path), &text, n);
        run_script_checked(&path, &text, &ops, RevealMode::Line);
        run_script_checked(&path, &text, &ops, RevealMode::Element);
    }
}

/// (c) One patch-sufficiency spot check per doc: prepend a strikethrough
/// span and confirm every byte outside the patch styles identically.
#[test]
fn corpus_patch_sufficiency() {
    let insert = "~~x~~ ";
    for path in corpus_files() {
        let text = read(&path);
        let mut e = Engine::new(&text, EngineOptions::default());
        let old: Vec<_> = e.segments_in(ByteRange::new(0, e.len_bytes())).to_vec();

        let patch = e.apply_edit(ByteRange::new(0, 0), insert).unwrap();
        let new: Vec<_> = e.segments_in(ByteRange::new(0, e.len_bytes())).to_vec();

        common::assert_patch_sufficient(
            e.text(),
            &old,
            &new,
            &patch.dirty,
            0,
            insert.len() as u32,
            insert.len() as i64,
        );
    }
}
