# Perch

Perch is an open, peer-to-peer data platform about products, prices, sellers, and reviews. It does not host sales. It shows consumers what is known about a product and who said it.

Every record is signed by its author and identified by its hash. Ratings, rankings, and moderation are computed on the user's device from public records and a published policy. Anyone can run a node, copy the full dataset, or fork the software.

## Where to read next

| Document | Contents |
|---|---|
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | The contract: guarantees, record format, sync, views, APIs, limits. Start here. |
| [`docs/data-model.md`](docs/data-model.md) | Records, canonical JSON, signatures, and how to verify a record with standard tools |
| [`docs/test-vectors.json`](docs/test-vectors.json) | Fixed records, receipts, and invalid inputs for other implementations |
| [`docs/views.md`](docs/views.md) | Exact rules for every computed number: ratings, facts, prices, moderation, search |
| [`docs/sync-protocol.md`](docs/sync-protocol.md) | Peer-to-peer sync messages, admission tiers, convergence, measurements |
| [`docs/api.md`](docs/api.md) | Node HTTP API, command line, operator controls, TLS |
| [`docs/threat-model.md`](docs/threat-model.md) | Threats, mitigations, residual risks, and the test that covers each guarantee |
| [`docs/identity.md`](docs/identity.md) | Identity and domain attestors, revocation, path to anonymous buyers |
| [`docs/verified-purchase.md`](docs/verified-purchase.md) | Purchase proofs compared; why receipts are seller-signed tokens |
| [`docs/moderation.md`](docs/moderation.md) | Flag thresholds with numbers, alternatives, and the v1.1 and v2 plan |
| [`docs/ux.md`](docs/ux.md), [`docs/ux-review.md`](docs/ux-review.md) | Trust UI rules, screen specs, and the usability review |
| [`extension/README.md`](extension/README.md) | What the browser extension shows and what leaves the browser |

## Layout

| Path | Purpose |
|---|---|
| `crates/perch-core` | Records, canonical JSON, keys, signatures, receipts, schemas, product keys |
| `crates/perch-store` | Storage with indexes, fork detection, body removal, light (sparse) mode, JSONL files |
| `crates/perch-sync` | Sync protocol as a state machine without I/O, plus a multi-node simulator |
| `crates/perch-views` | Deterministic views and the policy |
| `crates/perch-node` | Native node: HTTP API, WebSocket sync, persistence |
| `crates/perch-wasm` | Browser client (`PerchClient`) compiled to WebAssembly |
| `crates/perch-demo` | Deterministic signed demo dataset |
| `crates/perch-tests` | Adversarial tests with an independent encoder and signer |
| `web/` | Web app: HTML, CSS, and JavaScript modules, no dependencies |
| `extension/` | Browser extension (Manifest V3) |
| `tests/live`, `tests/browser` | Local network script and headless-browser tests |

## Quick start

Requirements: Rust 1.85 or later, the `wasm32-unknown-unknown` target, [`wasm-bindgen-cli`](https://rustwasm.github.io/wasm-bindgen/) 0.2.129, and Node.js 22 with [Playwright](https://playwright.dev) for browser tests.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked

scripts/build-wasm.sh          # builds the browser client into web/pkg
tests/live/run.sh              # 3 local nodes with demo data; open http://127.0.0.1:7701/
```

Run one node:

```sh
cargo run --release -p perch-node -- demo --out demo.jsonl --policy-out web/default-policy.json
cargo run --release -p perch-node -- serve --listen 127.0.0.1:7700 --data ./data \
    --import demo.jsonl --policy web/default-policy.json --web ./web
```

## Tests

`scripts/test-all.sh` runs every layer in order:

| Layer | Command | Tests |
|---|---|---|
| Unit, property, golden, simulation | `cargo test --workspace --exclude perch-node --exclude perch-tests` | Rust |
| Adversarial | `cargo test -p perch-tests` | Rust |
| Live multi-process | `cargo test -p perch-node` | Real node processes over WebSocket and TLS |
| Wasm API | `node crates/perch-wasm/tests/node-api.mjs` | Node.js |
| Browser | `node tests/browser/{mock-smoke,run,extension}.mjs` | Headless Chromium |

## License

MIT or Apache-2.0, at your option.
