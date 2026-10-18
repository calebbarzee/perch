#!/usr/bin/env bash
# Run every Perch test layer in order. Stops at the first failing layer.
#
# Layers:
#   1. Unit, property, and simulation tests (all crates)
#   2. Adversarial tests (perch-tests)
#   3. Live multi-process tests (perch-node integration tests)
#   4. Wasm build
#   5. Browser tests (headless Chromium via Playwright)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

step() { printf '\n=== %s ===\n' "$1"; }

step "1. Unit, property, and simulation tests"
cargo test --workspace --exclude perch-node --exclude perch-tests

step "2. Adversarial tests"
cargo test -p perch-tests

step "3. Live multi-process tests"
cargo test -p perch-node

step "4. Wasm build"
scripts/build-wasm.sh

step "5. Browser tests"
cargo build -p perch-node
NODE_PATH="${NODE_PATH:-/opt/npm-tools/node_modules}" node tests/browser/run.mjs

printf '\nAll test layers passed.\n'
