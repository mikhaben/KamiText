#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

# Builds KamiCore.xcframework from the Rust staticlib for the three Apple targets.
# Requires: rustup targets installed (aarch64-apple-ios, aarch64-apple-ios-sim,
# aarch64-apple-ios-macabi) and the cbindgen-generated header at core/include/.

CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
CORE_DIR="../../core"
LIB=libkamitext.a
HEADERS="$CORE_DIR/include"
OUT=KamiCore.xcframework

TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-ios-macabi aarch64-apple-darwin)

echo "==> rust release builds"
for t in "${TARGETS[@]}"; do
  # macabi is tier-2 without prebuilt std on some channels; -Zbuild-std not needed on stable 2026 toolchains
  (cd "$CORE_DIR" && "$CARGO" build --release --target "$t")
done

echo "==> verify header"
test -f "$HEADERS/kami_core.h" || { echo "missing $HEADERS/kami_core.h — run cbindgen first"; exit 1; }

echo "==> assemble $OUT"
rm -rf "$OUT"
ARGS=()
for t in "${TARGETS[@]}"; do
  ARGS+=(-library "$CORE_DIR/target/$t/release/$LIB" -headers "$HEADERS")
done
xcodebuild -create-xcframework "${ARGS[@]}" -output "$OUT"

echo "==> done: $(pwd)/$OUT"
