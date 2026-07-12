# Contributing

## Prerequisites

- Rust via [rustup](https://rustup.rs), with the four Apple targets:
  ```sh
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-ios-macabi aarch64-apple-darwin
  ```
- Full Xcode (not just the Command Line Tools) — assembling the xcframework calls `xcodebuild -create-xcframework`.

## Build & test

Rust, from `core/`:

```sh
cargo test          # unit, golden, differential proptest, pseudo-fuzz, FFI misuse
cargo clippy --all-targets -- -D warnings
```

Swift needs the xcframework built once, and again after any Rust change:

```sh
./bindings/swift/build-xcframework.sh
```

Then, from `bindings/swift/KamiTextKit/`:

```sh
swift test                 # conformance + regression tests, default budget
KAMI_FUZZ=1 swift test     # full differential-fuzz budget (4 seeds × 120 iterations vs. the default 1 × 40)
```

## Conformance fixtures

`fixtures/*.json` are frozen goldens — the contract that proves an adapter is behaviorally identical to the reference engine. If a test fails against a fixture, the fixture is right until proven otherwise. Regenerating one (`cargo run --bin export-fixtures` from `core/`) to make a failing test pass is never a fix — it just launders a regression into the golden set. Fixtures only change when you deliberately change engine output and the PR explains why.

## Code style

Match the existing doc-comment style: a `//!` module summary stating what the module owns, `///` item docs stating the exact behavior an item guarantees. The engine invariants in AGENTS.md plus the conformance fixtures in `fixtures/` are the behavioral contract — a change that would contradict them needs its invariant updated (and fixtures regenerated) in the same PR, with the reasoning in the description.

## Pull requests

- Explain what changed and why, citing the AGENTS.md invariant or conformance fixture that behavior traces to.
- The maintainer cuts releases — PRs don't touch version numbers or changelogs.
