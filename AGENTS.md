# Kami — Engine + Adapter Monorepo

Portable Markdown editor engine for hidden-syntax live editing (Obsidian-style: markers conceal on inactive lines, reveal at the caret). Rust core decides all styling/conceal/typing behavior; platform adapters are dumb appliers. The behavioral contract is the **Invariants** section below plus `fixtures/` — an adapter that replays every fixture correctly is conformant; if you change segment/element output, the fixtures are regenerated and committed with the change.

## Build & Dev Commands

Rust (from `core/`) — `cargo` may live at `~/.cargo/bin/cargo` if not on PATH:

- `cargo test` — all suites (unit, golden, differential proptest, pseudo-fuzz, FFI misuse, corpus)
- `cargo test --test corpus` — real-markdown corpus sweep (`corpora/*.md`); `KAMI_CORPUS_FULL=1` unlocks full-length edit scripts
- `cargo clippy --all-targets -- -D warnings` — required-clean gate
- `cargo bench` — criterion keystroke bench (synthetic + `corpus-fpb` real-doc variant); gate: `apply_edit` p50 < 3 ms @ 250 KB (p99 < 8 ms via `cargo run --release --example profile`)
- `cargo run --bin export-fixtures` — regenerates `fixtures/*.json`; run whenever segment/element output changes, and commit the result

Swift (from `bindings/swift/KamiTextKit/`) — requires the xcframework bootstrap first:

- `../build-xcframework.sh` — Rust release builds for 4 Apple targets → `KamiCore.xcframework`. Rerun after ANY Rust change or the package links a stale core
- `swift build` / `swift test` — native macOS build + conformance/regression tests
- `swift run KamiDemoMac` — macOS demo window; `--selftest` = headless gate (seed + keystroke sync + reveal assertions, exits 0)
- Catalyst/iOS test destinations: `xcodebuild test -scheme KamiTextKit -destination 'platform=macOS,variant=Mac Catalyst' -derivedDataPath .dd CODE_SIGNING_ALLOWED=NO`

## Project Structure

```
core/
├── Cargo.toml                  # crate kamitext; staticlib; pulldown-cmark 0.13 + bitflags pinned
├── cbindgen.toml               # C header generation config (header regen command in its comment)
├── src/
│   ├── lib.rs                  # module wiring; #![deny(unsafe_code)] with ffi-only allow
│   ├── types.rs                # contract types: ByteRange, Kind bitflags, Segment, Element, Patch, EditPlan, KamiError
│   ├── document.rs             # text buffer, strict edit validation (never repairs), \n line index (CRLF-aware content ranges)
│   ├── offsets.rs              # checkpointed byte↔UTF-16 index, ~4 KB scalar-aligned chunks, incremental splice on edit
│   ├── parse.rs                # pulldown OffsetIter walk → content paints, marker paints (gap technique + targeted scans), elements
│   ├── analysis.rs             # sweep-line flatten (packed-u64 events) → sorted/covering/coalesced raw segments + UTF-16 assignment
│   ├── conceal.rs              # reveal region (None/Line/Block) + conceal resolution against selection
│   ├── patch.rs                # old-vs-new segment diffs → segment-aligned dirty ranges (edit + same-doc variants)
│   ├── behaviors.rs            # newline continuation / exit-on-empty / task toggle → EditPlans
│   ├── engine.rs               # Engine facade: reparse-per-edit, arena reuse, selection mapping, Send-not-Sync
│   ├── ffi.rs                  # C ABI — the ONLY unsafe module; catch_unwind + poisoning, generation counter, arena views
│   └── bin/export-fixtures.rs  # conformance fixture writer (hand-rolled JSON, byte-stable output)
├── tests/
│   ├── common/mod.rs           # shared helpers: assert_invariants, ATOMS, FuzzRng/next_valid_op, assert_patch_sufficient
│   ├── golden.rs               # hand-reviewed segment/element/patch expectations
│   ├── differential.rs         # incremental ≡ fresh proptest over random op sequences
│   ├── element_reveal.rs       # Element-mode boundary matrix + patch-completeness proptest
│   ├── corpus.rs               # real-markdown sweep over corpora/*.md (invariants + differential, both reveal modes)
│   ├── pseudo_fuzz.rs          # seeded 120k-op fuzz incl. deliberately invalid calls
│   └── ffi_misuse.rs           # NULL/UTF-8/poisoning/arena-scribble abuse of the C ABI
├── benches/                    # criterion: keystroke.rs (synthetic 5–250 KB + corpus-fpb real doc), selection_move.rs
├── examples/                   # probe.rs (pulldown range semantics), profile.rs (phase timings) — dev tools
└── include/                    # kami_core.h (generated) + module.modulemap
bindings/swift/
├── build-xcframework.sh        # cargo × {ios, ios-sim, macabi, darwin} → xcframework (NOT committed; bootstrap step)
└── KamiTextKit/
    ├── Package.swift           # binaryTarget KamiCore + KamiTextKit + KamiDemoMac + tests; iOS 17 / Catalyst 17 / macOS 14
    ├── Sources/KamiTextKit/
    │   ├── KamiEngine.swift    # @MainActor FFI wrapper; ABI check at init; copies every arena view before any next call
    │   ├── KamiTypes.swift     # Swift mirrors of segments/elements/plans/kind bits
    │   ├── KamiPlatform.swift  # UIKit/AppKit shim: KamiFont/KamiColor aliases, trait-adding, semantic colors
    │   ├── KamiTheme.swift     # Kind set + concealed → NSAttributedString attrs + checkedTaskOverlayAttributes() hook; DefaultKamiTheme (0.01pt+clear conceal trick)
    │   ├── KamiTextStorageApplier.swift  # segments→storage; task-overlay widening; detached build for full seeds; attr memo
    │   └── KamiTextSync.swift  # reusable host driver: willChange/didChange/selectionChanged state machine, IME/undo/desync recovery, setTheme
    ├── Sources/KamiDemoMac/main.swift    # AppKit demo window (NSTextView TextKit 2) + --selftest; reference host wiring incl. Cmd+T re-theme
    └── Tests/KamiTextKitTests/ # fixture conformance replay, applier/memo/setTheme regressions, marked-text desync, differential fuzz through a live view
fixtures/                       # 32 conformance fixtures — adapters replaying these correctly are conformant
corpora/                        # real-markdown regression corpus + manifest.json (sources, licenses, marker density)
PLATFORM_BUGS.md                # AppKit/TextKit quirk ledger: status, workarounds, retest dates, negative findings
```

## Architecture

One edit flows: `Engine::apply_edit` validates (reject, never repair) → `Document` splices text + patches the UTF-16 checkpoint index → full reparse (`parse` → `flatten` → `assign_utf16`) → `conceal::resolve` against the reveal region → `patch::diff_after_edit` emits dirty ranges. Selection changes skip the reparse and diff conceal state only. Swift side: `KamiTextSync` translates platform delegate events into engine calls and hands patches to `KamiTextStorageApplier`, which re-fetches segments for dirty ranges and rewrites attributes.

## Invariants (do not break)

- **Text is never mutated for display** — concealment is attributes only (0.01 pt font + clear color); indices stay 1:1 with the source.
- **Byte offsets (UTF-8) are the canonical coordinate system**; UTF-16 ranges are provided by core, never recomputed by adapters.
- **Patches never under-report** — every byte whose styling changed is inside a dirty range; adapters may restyle only the patch. If you change segment output, the differential proptest and patch-sufficiency tests must still pass, and fixtures must be regenerated.
- **FFI arena views die at the next call** on the same engine — Swift copies out immediately; never hold a `Kami*` raw struct across engine calls. Read-only observers (`kami_generation`, `kami_len_*`, `kami_last_error_message`) don't bump the generation.
- **Determinism**: identical input ⇒ byte-identical output. No HashMap iteration order, no floats, no time/RNG in `core/src/`.
- Engine construction from invalid UTF-8/options returns NULL; panics poison the engine (`KAMI_ERR_POISONED` afterward).

## Gotchas

- Platform-behavior bugs and their workarounds are tracked in `PLATFORM_BUGS.md` (status, retest dates, negative findings) — check it before investigating an AppKit/TextKit oddity.
- SourceKit shows phantom "Cannot find type / No such module" errors in package sources until the first `swift build` — trust the compiler, not the IDE diagnostics.
- Files using `NSAttributedString.Key.font` etc. need the `#if canImport(UIKit) import UIKit #else import AppKit` block themselves — typealiases cross the module, extension visibility doesn't.
- Never touch `.layoutManager` on a text view (silently downgrades TextKit 2 → 1); `.textStorage` is safe.
- AppKit undo fires NO edit delegate — only a selection change; `KamiTextSync.selectionChanged` reseeds on length desync, and a length-preserving undo heals on the next length change (see its doc comment before "fixing" this).
- AppKit hosts should implement the **plural** `shouldChangeTextInRanges` (AppKit then calls only it) and stash single-range edits only — see `KamiDemoMac/main.swift`.
- The parser is pulldown-cmark, NOT `swift-markdown` — offset semantics differ across markdown parsers, and pulldown emits byte offsets directly, which is what the whole coordinate system assumes.
