# Kami

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE) [![Swift 6.0](https://img.shields.io/badge/Swift-6.0-F05138.svg)](https://swift.org) [![Rust 2024](https://img.shields.io/badge/Rust-2024-000000.svg)](https://www.rust-lang.org)

A portable Markdown **editor engine** for hidden-syntax live editing — the Obsidian/Bear-style experience where `**bold**` renders bold and the markers vanish until your caret reaches them. The engine decides *what should look like what*; thin platform adapters paint it. A small Rust core behind a C ABI, with adapters that stay dumb enough to port anywhere.

## Why

I wanted hidden-syntax live editing for a notes app I was building and couldn't find an open-source engine that ships it — every polished editor keeps this part proprietary. So I built one, and open-sourced it so nobody else has to solve this twice.

## Features

- **Hidden-syntax live editing** — markers conceal on inactive lines, reveal at the caret; the text is never mutated for display, so byte offsets always match the file on disk
- **CommonMark + GFM** (tables, task lists, strikethrough — each toggleable) via pulldown-cmark
- **Patch-based updates** — each edit returns exactly the byte ranges whose styling changed; adapters restyle only those, so keystroke cost stays flat regardless of document size
- **UTF-16 offset conversion built in** — Apple, Kotlin, and JS adapters never do their own emoji math
- **Typing behaviors as data** — list/quote/task continuation and checkbox toggling come back as edit plans the host applies through its own undo stack
- **Conformance fixtures** — 32 golden JSON fixtures; an adapter that replays them correctly is behaviorally identical to the reference

## Measured (Apple Silicon, release, 2026-07)

| Document | `apply_edit` (engine) | `apply_edit` + segment fetch |
|---|---|---|
| 5 KB synthetic | 27 µs | 27 µs |
| 20 KB synthetic | 116 µs | 118 µs |
| 50 KB synthetic | 293 µs | 282 µs |
| 100 KB synthetic | 568 µs | 577 µs |
| 250 KB synthetic | 1.49 ms | 1.52 ms |
| 198 KB real doc (`corpora/fpb-langs.md`, 0.50 marker density) | 1.48 ms | 1.47 ms |

Column definitions: `apply_edit` is the engine keystroke (validate → splice → reparse → conceal → diff, patch returned) — the engine's keystroke budget (p50 < 3 ms, p99 < 8 ms @ 250 KB) applies to this column, and measures p50 1.49 ms / p99 1.75 ms over 2,000 keystrokes (`cargo run --release --example profile`). The second column adds the adapter's segment re-fetch over the patch's dirty ranges (criterion medians, `cargo bench`). Adapter attribute writes are O(patch) on top. Opening a document is a one-time full parse: `Engine::new` at 250 KB measures ~2.1 ms.

## Quick Start

The Swift package binds a prebuilt `KamiCore.xcframework` that is **not** committed — build it once:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-ios-macabi aarch64-apple-darwin
./bindings/swift/build-xcframework.sh
```

Then, from `bindings/swift/KamiTextKit/`:

```sh
swift build                      # library
swift test                       # conformance + regression tests
swift run KamiDemoMac            # macOS demo window — type and watch markers conceal/reveal
swift run KamiDemoMac --selftest # headless sync/reveal checks
```

Rust side, from `core/`: `cargo test`, `cargo bench`, `cargo run --bin export-fixtures`.

## Using it in your app

Add the `KamiTextKit` package, then wire three delegate calls into `KamiTextSync` (works with `UITextView` and `NSTextView` — it only speaks `NSTextStorage` + `NSRange`):

1. `shouldChangeTextIn` → `willChange(...)`
2. `textDidChange` → `didChange(...)`
3. `textViewDidChangeSelection` → `selectionChanged(...)`

Seed once with `seed(text:storage:selectedRange:)`. See `Sources/KamiDemoMac/main.swift` for a complete ~200-line host, including the IME and undo caveats documented on `KamiTextSync`.

## Theming

The engine emits *semantics* (a composed kind set per run); your app decides pixels. Implement `KamiTheme` — delegate to `DefaultKamiTheme` and override the runs you care about:

```swift
struct MyTheme: KamiTheme {
    private let base = DefaultKamiTheme()

    func attributes(for kinds: KamiKindSet, concealed: Bool) -> [NSAttributedString.Key: Any] {
        var attrs = base.attributes(for: kinds, concealed: concealed)
        if kinds.contains(.strong), !concealed { attrs[.foregroundColor] = UIColor.systemRed }
        return attrs
    }

    // Checked-task content styling is a separate hook (checked-ness is element
    // state, not a kind bit). Default is strikethrough + dim; return [:] to disable.
    func checkedTaskOverlayAttributes() -> [NSAttributedString.Key: Any] {
        [.backgroundColor: UIColor.systemYellow.withAlphaComponent(0.3)]
    }
}

let sync = KamiTextSync(theme: MyTheme())
```

Kind bits compose, so bold-inside-a-heading is `[.heading1, .strong]` and can be styled distinctly. Interactive widgets (real checkbox views, tappable links) are adapter-side work built on the `Element` stream the engine already provides — the styling hooks above are the pure-visual tier.

## Project Structure

```
core/                  # kamitext: the Rust engine (parse → segments → conceal → patches)
├── src/               # document, offsets, parse, analysis, conceal, patch, behaviors, engine, ffi
├── tests/             # golden, differential (proptest), corpus sweep, pseudo-fuzz, FFI-misuse suites
├── benches/           # criterion keystroke + selection benchmarks (synthetic 5–250 KB + real corpus doc)
└── include/           # cbindgen-generated C header + module map
bindings/swift/
├── build-xcframework.sh   # Rust → KamiCore.xcframework (4 Apple slices)
└── KamiTextKit/           # Swift package: engine wrapper, theme, applier, sync driver, macOS demo
fixtures/              # conformance fixtures (JSON) — the adapter compatibility contract
corpora/               # real-markdown regression corpus; sources and licenses in manifest.json
PLATFORM_BUGS.md       # AppKit/TextKit quirk ledger: workarounds, retest dates, negative findings
```

## Tech

Rust (pulldown-cmark, bitflags; proptest + criterion for tests/benches) · C ABI via cbindgen · Swift 6 (TextKit 2, SPM) targeting iOS 17+, Mac Catalyst 17+, macOS 14+.

## License

MIT — see [LICENSE](LICENSE). The test corpus in `corpora/` contains third-party documents; their sources and licenses are recorded in [`corpora/manifest.json`](corpora/manifest.json).
