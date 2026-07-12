## What & why

<!-- What changed, and why the engine or adapter needed to behave differently. If behavior changed, name the invariant (AGENTS.md) or fixtures it touches. -->

## How tested

<!-- Which suites you ran, and anything you exercised manually (e.g. the macOS demo). -->

- [ ] `cargo test` (from `core/`)
- [ ] `swift test` (from `bindings/swift/KamiTextKit/`)
- [ ] `KAMI_FUZZ=1 swift test` (full fuzz budget — run if the change touches engine, sync, or applier internals)

## Checklist

- [ ] `cargo test` is green
- [ ] `swift test` is green
- [ ] `fixtures/*.json` unchanged, or changed deliberately with rationale above (regenerating a fixture to make a failing test pass is not a fix)
- [ ] No version numbers or changelog entries touched (maintainer cuts releases)
