#!/usr/bin/env bash
# Build perch-wasm and generate JS bindings into web/pkg.
#
# Usage: scripts/build-wasm.sh [--debug]
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

wasm-bindgen \
  --target web \
  --out-dir "$ROOT/web/pkg" \
  "$ROOT/target/$TARGET/$PROFILE/perch_wasm.wasm"

echo "wasm bindings written to web/pkg"
