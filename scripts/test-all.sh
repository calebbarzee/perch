#!/usr/bin/env bash
# Run every Perch test layer in order. Stops at the first failing layer.
#
# Layers:
#   1. Unit, property, golden, and simulation tests (all crates except node and adversarial)
#   2. Adversarial tests (perch-tests)
#   3. Live multi-process tests (perch-node integration tests)
#   4. Wasm build and the wasm API test in Node
#   5. Browser tests in headless Chromium: app on a mock client, app against
#      live nodes, and the browser extension against live nodes
#
# Needs: Rust, wasm-bindgen CLI, Node.js, Playwright with Chromium.
# NODE_PATH must reach Playwright (default: /opt/npm-tools/node_modules).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export NODE_PATH="${NODE_PATH:-/opt/npm-tools/node_modules}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

step() { printf '\n=== %s ===\n' "$1"; }

step "1. Unit, property, golden, and simulation tests"
cargo test --workspace --exclude perch-node --exclude perch-tests

step "2. Adversarial tests"
cargo test -p perch-tests

step "3. Live multi-process tests"
cargo test -p perch-node

step "4. Wasm build and wasm API test"
scripts/build-wasm.sh
cargo run -q -p perch-wasm --example demo_jsonl -- "$TMP/demo.jsonl" "$TMP/policy.json"
PERCH_DEMO="$TMP/demo.jsonl" PERCH_DEMO_POLICY="$TMP/policy.json" node crates/perch-wasm/tests/node-api.mjs

step "5. Browser tests"
cargo build -p perch-node
node tests/browser/mock-smoke.mjs
node tests/browser/run.mjs
node tests/browser/extension.mjs

printf '\nAll test layers passed.\n'
