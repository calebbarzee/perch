#!/usr/bin/env bash
# Build perch-wasm, generate JS bindings into web/pkg, and stage the files the
# browser extension needs.
#
# Usage: scripts/build-wasm.sh [--debug]
#
# Outputs (all generated, not committed):
#   web/pkg/                    perch_wasm.js, perch_wasm_bg.wasm, typings
#   web/pkg/SHA256SUMS          SHA-256 of each file in web/pkg
#   extension/pkg/              copy of web/pkg
#   extension/default-policy.json, extension/fonts/   copies from web/
#
# If the wasm32-unknown-unknown target is installed (rustup target add
# wasm32-unknown-unknown), a normal build is used. Otherwise the standard
# library is compiled from source with -Zbuild-std. That fallback needs the
# rust-src component in the sysroot and RUSTC_BOOTSTRAP=1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="release"
CARGO_PROFILE_FLAG="--release"
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE="debug"
  CARGO_PROFILE_FLAG=""
fi

TARGET=wasm32-unknown-unknown
EXTRA=()
if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
  export RUSTC_BOOTSTRAP=1
  EXTRA=(-Zbuild-std=std,panic_abort)
fi

cd "$ROOT"
# getrandom 0.3+/0.4 needs the wasm_js backend selected by cfg on this target.
export RUSTFLAGS="${RUSTFLAGS:-} --cfg getrandom_backend=\"wasm_js\""
cargo build -p perch-wasm --target "$TARGET" $CARGO_PROFILE_FLAG "${EXTRA[@]}"

rm -rf "$ROOT/web/pkg"
wasm-bindgen \
  --target web \
  --out-dir "$ROOT/web/pkg" \
  "$ROOT/target/$TARGET/$PROFILE/perch_wasm.wasm"
(cd "$ROOT/web/pkg" && sha256sum -- * > SHA256SUMS)

# Browser extension: same wasm module, same bundled policy.
rm -rf "$ROOT/extension/pkg" "$ROOT/extension/fonts"
cp -R "$ROOT/web/pkg" "$ROOT/extension/pkg"
cp "$ROOT/web/default-policy.json" "$ROOT/extension/default-policy.json"
mkdir -p "$ROOT/extension/fonts"
cp "$ROOT/web/fonts/atkinson-hyperlegible-next-latin-wght-normal.woff2" "$ROOT/web/fonts/OFL-atkinson-hyperlegible-next.txt" "$ROOT/extension/fonts/"

echo "wasm bindings written to web/pkg and extension/pkg"
grep perch_wasm_bg.wasm "$ROOT/web/pkg/SHA256SUMS"
