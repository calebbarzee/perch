# Perch architecture

Perch is an open, peer-to-peer data platform about products, prices, sellers, and reviews. It does not host sales. It tells consumers what is known about a product and who said it.

This document is the contract between all Perch components. Detailed specifications live in [`docs/`](docs/). When a detail document conflicts with this file, this file wins until it is updated.

## 1. Goal: trust by construction

Users should be able to trust Perch because of how it is built, not because of who runs it. Each guarantee below is enforced by a mechanism that any user can check.

| Guarantee | Mechanism |
|---|---|
| Every claim has a known author | Each record is signed with the author's [Ed25519](https://www.rfc-editor.org/rfc/rfc8032) key. |
| No one can alter a claim | Each record is identified by the [SHA-256](https://csrc.nist.gov/pubs/fips/180-4/upd1/final) hash of its content. Any change produces a different ID and breaks the signature. |
| No one can silently edit or delete history | Each author publishes an append-only log. Each record names the hash of the previous one. Two different records at the same position prove misbehavior. |
| No one controls the data | The full dataset is a file of signed records. Anyone can copy it, run a node, or fork the software. |
| No one can buy ranking | Ratings, rankings, and moderation are computed on the user's device by published, deterministic code. Inputs are public records only. |
| Moderation is visible | Moderation hides records according to a published policy. It never deletes them. Users can inspect and change the policy. |

## 2. System overview

```
             signed records (JSON)
  ┌──────────┐   WebSocket sync   ┌──────────┐
  │ Browser  │◄──────────────────►│  Node A  │◄──┐
  │ (wasm)   │                    └──────────┘   │ WebSocket sync
  └──────────┘                    ┌──────────┐   │
  ┌──────────┐   HTTP API (read)  │  Node B  │◄──┘
  │ 3rd-party│───────────────────►└──────────┘
  │ client   │
  └──────────┘
```

- **Records** are the only data. Everything else is derived from them.
- **Nodes** are native processes that store, verify, and relay records. Anyone can run one. Nodes are equal; there is no central server.
- **Browser clients** run the same Rust code compiled to WebAssembly (wasm). They verify every record and compute every view locally. They connect to one or more nodes over WebSocket.
- **Views** are pure functions from (records, policy) to results. Examples: merged product facts, rating summaries, price history, moderation status, search ranking.

## 3. Components

| Crate / folder | Purpose | Runs on |
|---|---|---|
| `crates/perch-core` | Record types, canonical encoding, keys, signatures, isolated validation, product keys | native, wasm |
| `crates/perch-store` | `Store` trait, in-memory store with indexes, append-only JSONL persistence, log continuity and fork detection | native, wasm |
| `crates/perch-sync` | Sync protocol as a state machine with no I/O ("sans-IO"); multi-node simulation tests | native, wasm |
| `crates/perch-views` | Deterministic views and the `Policy` type | native, wasm |
| `crates/perch-demo` | Deterministic demo and test dataset generator | native, wasm |
| `crates/perch-node` | Native node binary: WebSocket sync, HTTP API, persistence, CLI, serves the web app | native |
| `crates/perch-wasm` | `wasm-bindgen` bindings: the `PerchClient` JavaScript API | wasm |
| `web/` | Official web app. Plain HTML, CSS, and JavaScript modules over `PerchClient` | browser |
| `extension/` | Browser extension that shows Perch data on retailer pages | browser |
| `crates/perch-tests` | Cross-crate integration and adversarial tests | native |
| `tests/` | Live multi-process tests and browser tests ([Playwright](https://playwright.dev)) | native, browser |

Dependency direction: `core ← store ← views`, `core ← store ← sync`, `views + sync ← node`, `views + sync ← wasm`. `perch-demo` depends on `core` only.

## 4. Records

### 4.1 Envelope

A record is a JSON object. All records share this envelope.

| Field | Type | Rule |
|---|---|---|
| `v` | integer | Always `1`. |
| `author` | string | Ed25519 public key, 64 lowercase hex characters. |
| `seq` | integer | Position in the author's log. Starts at `1`. Increases by exactly 1. |
| `prev` | string or `null` | ID of the author's record at `seq - 1`. `null` if and only if `seq` is `1`. |
| `ts` | integer | Time claimed by the author, Unix milliseconds. Not trusted. |
| `kind` | string | Record type. Matches `^[a-z][a-z0-9_.]{0,31}$`. |
| `replaces` | string | Optional. Omitted when absent. ID of an earlier record by the same author and of the same kind that this record supersedes. |
| `body` | object | Kind-specific content (section 4.4). |
| `sig` | string | Ed25519 signature, 128 lowercase hex characters. |

Unknown envelope fields make a record invalid.

### 4.2 Canonical encoding, ID, and signature

The canonical form of a record is its envelope without `sig`, encoded as JSON with these rules. The rules are a subset of [RFC 8785 (JSON Canonicalization Scheme)](https://www.rfc-editor.org/rfc/rfc8785).

- UTF-8, no whitespace outside strings.
- Object keys sorted by their UTF-8 bytes. Duplicate keys are invalid.
- Numbers are integers in the range ±(2^53 − 1). No fractions, no exponents, no `-0`.
- Strings escape only `"`, `\`, and control characters below U+0020. Use `\b \t \n \f \r` where defined, otherwise `\u00xx` with lowercase hex.

Definitions:

- `canonical = canonical_json(envelope without sig)`
- `id = lowercase_hex(sha256(canonical))`
- `sig = ed25519_sign(secret_key, "perch/record/v1\n" || canonical)`

Implementations must keep and relay the record exactly as signed, including body fields they do not understand. They must never re-encode a record from a typed struct for verification.

Maximum size of `canonical` is 65,536 bytes.

### 4.3 Validity levels

Validity has three levels. Keeping log acceptance independent of body schemas lets old and new software agree on which records exist.

| Level | Checked by | Rules | If it fails |
|---|---|---|---|
| Isolated | `perch-core` | Envelope fields and types, canonical size, `kind` format, `body` is an object, signature | Record is rejected and never stored or relayed. |
| Log | `perch-store` | `seq` and `prev` continue the author's log | Gap: held or requested by sync. Conflict: stored as fork evidence; author marked forked. |
| Semantic | `perch-views` | Body schema for the kind; `replaces` and `target` references; author permissions | Record stays stored and relayed. Views ignore it and can report why. |

Unknown kinds pass the isolated and log levels. Views ignore them.

### 4.4 Kinds (v1)

All text fields are UTF-8 strings. Lengths are in Unicode scalar values. `Id` means a record ID. `Author` means a 64-hex public key. `ProductKey` is defined in 4.5. `Price` is `{ "amount": integer ≥ 0 in the currency's minor unit, "currency": ISO 4217 code }`. Optional fields are omitted when absent.

| Kind | Author | Body |
|---|---|---|
| `profile` | anyone | `name` (1–100), `role` (`seller`, `buyer`, `tester`, `crawler`, `attestor`, `other`), `url`? (≤2048), `about`? (≤2000) |
| `attest.identity` | identity attestor | `subject` (Author), `level` (`person`, `business`), `country`? ([ISO 3166-1](https://www.iso.org/iso-3166-country-codes.html) alpha-2), `expires` (Unix ms) |
| `product` | anyone | `name` (1–300), `brand` (1–100), `gtin`? (8, 12, 13, or 14 digits with a valid check digit), `mpn`? (1–100), `category`? (≤200, lowercase, `/`-separated), `attributes`? (object, ≤64 entries, keys ≤64, values ≤500), `images`? (≤16 × `{ "url", "sha256"? }`), `description`? (≤5000) |
| `listing` | seller | `product` (ProductKey), `url` (≤2048, `https://`), `price` (Price), `availability` (`in_stock`, `out_of_stock`, `preorder`, `discontinued`), `condition` (`new`, `used`, `refurbished`), `title`? (≤300), `ships_to`? (list of alpha-2), `location`? (`{ "country", "region"? }`) |
| `observation` | anyone | `product` (ProductKey), `url` (≤2048), `price` (Price), `observed_at` (Unix ms), `listing`? (Id), `availability`? |
| `review` | anyone | `product` (ProductKey), `rating` (integer 1–5), `text` (1–10000), `title`? (≤200), `listing`? (Id), `receipt`? (Id) |
| `receipt` | seller of the listing | `listing` (Id), `buyer` (Author), `purchased_at` (Unix ms), `order_hash`? (64 hex, SHA-256 of the seller's order number) |
| `flag` | anyone | `target` (Id), `reason` (`fraud`, `counterfeit`, `misleading`, `spam`, `illegal`, `other`), `note`? (≤2000) |
| `appeal` | author of the target | `target` (Id), `note` (1–2000) |
| `retract` | author of the target | `target` (Id). Withdraws the author's own earlier record. |
| `retire` | anyone | `successor`? (Author). Ends the author's log. Records after it are ignored by views. |

Versioning: a record with `replaces` supersedes an earlier record. A chain of replacements has a **root ID**: the ID of the first record in the chain. Listings, products, and reviews are referred to by root ID. Views use the latest valid version and keep the history.

### 4.5 Product keys

A `ProductKey` is a string that groups records about the same product.

| Priority | Condition | Key |
|---|---|---|
| 1 | Product has `gtin` | `gtin:` + GTIN zero-padded to 14 digits |
| 2 | Product has `mpn` | `mpn:` + lowercase trimmed brand + `:` + lowercase trimmed MPN |
| 3 | Otherwise | `rec:` + root ID of the product record |

[GTIN](https://www.gs1.org/standards/id-keys/gtin) check digits are validated.

## 5. Logs and storage

- Each author has one log. A node stores a log as a complete prefix: it holds record `n` only if it holds records `1` to `n − 1`.
- A **fork** is two isolated-valid records by the same author with the same `seq` and different IDs. A node keeps the first record in the log and stores the second as evidence. The fork pair is proof that the author misbehaved. Sync relays fork evidence.
- Persistence is an append-only file of records, one canonical-JSON record per line ([JSON Lines](https://jsonlines.org)). The file is also the public export format. On start, a node replays the file.
- The `Store` trait hides storage. v1 has one implementation: in-memory indexes plus optional JSONL persistence. Browsers persist to IndexedDB later.

## 6. Sync

Detail: [`docs/sync-protocol.md`](docs/sync-protocol.md).

- Transport: WebSocket text frames carrying JSON messages. The state machine in `perch-sync` performs no I/O. Nodes, browsers, and tests drive it.
- Summary exchange: peers exchange **heads** (`author → (seq, id)`). Each side then requests the records it lacks with `want { author, from_seq }`.
- Live gossip: after the initial exchange, newly accepted records are pushed to connected peers. Duplicates are dropped by ID.
- Peer discovery: a node starts from a configured list of peers and exchanges known peer addresses.
- Goal: **convergence**. Connected nodes end with the same set of logs and fork evidence, so they compute the same views.

## 7. Views

Detail: [`docs/views.md`](docs/views.md).

A view is a pure function `view(store, policy) -> result`. The same records and the same policy give the same result on every platform. Views never read the clock; `policy.now` supplies the time.

### 7.1 Policy

```json
{
  "now": 1760000000000,
  "trusted_attestors": ["<author hex>"],
  "flag_threshold": 3,
  "flags_require_verified": true,
  "seller_hidden_listing_limit": 3
}
```

The official app ships a default policy. Forks and users may change it. Every view result is reproducible from (records, policy).

### 7.2 Identity and verification

An author is **verified** when an unexpired, unretracted `attest.identity` record names them as `subject` and its author is in `trusted_attestors`. Real-world identity is the v1 basis for sybil resistance. Anonymous buyers are a planned extension (see [`docs/threat-model.md`](docs/threat-model.md)).

### 7.3 Verified purchase

A review is a **verified purchase** when:

1. it references a `receipt`,
2. the receipt's `buyer` is the review's author,
3. the receipt's author is the author of the receipt's `listing`, and
4. that listing's `product` equals the review's `product`.

Other proofs (for example [DKIM](https://www.rfc-editor.org/rfc/rfc6376)-signed receipt emails) are researched in [`docs/verified-purchase.md`](docs/verified-purchase.md). A browser extension cannot prove a purchase by itself, because users control the code it runs.

### 7.4 Moderation

- A listing is **hidden** when the number of distinct, unretracted flags on it reaches `flag_threshold`. If `flags_require_verified` is true, only flags from verified authors count. Flags by the listing's own seller never count.
- An **appeal** is a public request from the seller. It does not change status by itself. Status changes when flaggers retract their flags.
- A seller is **restricted** when `seller_hidden_listing_limit` or more of their listings are hidden. Views mark all of a restricted seller's listings.
- An author with fork evidence is **forked**. Views mark the author and exclude their flags and reviews from aggregates.
- Hidden records remain in the data. The app shows that something is hidden and why.

### 7.5 View result shapes (JSON)

These shapes are the contract between `perch-views`, `perch-wasm`, the HTTP API, and the web app. Fields may be added. Fields may not be renamed without updating all consumers.

```text
Source       { author, record, verified: bool }
Fact<T>      { value: T, sources: [Source], conflicts: [{ value: T, sources: [Source] }] }

AuthorView   { id, name|null, role|null, url|null, about|null,
               verified: { level, country|null, attestor, expires }|null,
               status: "ok"|"forked"|"retired", restricted: bool,
               record_count, hidden_listing_count }

ListingView  { id (root id), latest (record id), seller (author id), seller_name|null,
               seller_verified: bool, seller_restricted: bool,
               product, title|null, url, price: { amount, currency },
               availability, condition, updated_ts,
               status: "visible"|"hidden"|"retracted",
               flags: { count, counted, reasons: { reason: n } }, appealed: bool,
               price_history: [{ ts, amount, currency, source: "seller"|"observation", record }] }

ReviewView   { id (root id), latest, author, author_name|null, author_verified: bool,
               verified_purchase: bool, rating, title|null, text, ts, listing|null }

RatingSummary { count, mean|null, histogram: [n1, n2, n3, n4, n5],
                verified_purchase: { count, mean|null, histogram },
                verified_identity: { count, mean|null, histogram } }

ProductView  { key, name: Fact<string>, brand: Fact<string>, category: Fact<string>|null,
               description: Fact<string>|null, gtin|null, mpn|null,
               attributes: { name: Fact<string> }, images: [{ url, sha256|null }],
               listings: [ListingView], reviews: [ReviewView], ratings: RatingSummary,
               price_range: { currency: { min, max, median } } }

SearchHit    { key, name, brand, category|null, score, why: [string],
               rating_mean|null, rating_count, min_price: { amount, currency }|null }

Stats        { records, authors, products, listings, reviews, forks, invalid }
```

`mean` values are numbers rounded to 2 decimal places. All other numbers are integers.

## 8. Interfaces

### 8.1 HTTP API (official public API, served by `perch-node`)

Detail: [`docs/api.md`](docs/api.md). All responses are JSON unless noted.

| Method and path | Result |
|---|---|
| `GET /v1/status` | Node info, record counts, peers |
| `POST /v1/records` | Submit signed records (JSON array or JSONL). Returns per-record outcome. |
| `GET /v1/records/{id}` | One signed record |
| `GET /v1/authors/{id}` | `AuthorView` |
| `GET /v1/authors/{id}/log?from=&limit=` | Signed records of one log |
| `GET /v1/search?q=` | `[SearchHit]` |
| `GET /v1/products/{key}` | `ProductView` |
| `GET /v1/listings/{id}` | `ListingView` |
| `GET /v1/stats` | `Stats` |
| `GET /v1/export.jsonl` | Entire dataset as JSON Lines |
| `GET /v1/sync` (WebSocket) | Sync protocol |

Nodes never hold user secret keys. Clients sign records locally and submit them.

### 8.2 JavaScript API (`PerchClient`, from `perch-wasm`)

All methods that return data return plain JSON-compatible objects using the shapes in 7.5.

```text
PerchClient.create()                          -> PerchClient
client.generateKey()                          -> author hex
client.importKey(secretHex) / exportKey()     -> secret hex (32-byte seed)
client.author()                               -> author hex | null
client.publish(kind, body, { replaces?, ts? }) -> { id, record }
client.ingest(recordsOrJsonl)                 -> { inserted, duplicate, fork, gap, invalid, errors: [] }
client.connect(wsUrl) / disconnect(wsUrl)     -> Promise
client.peers()                                -> [{ url, state, sent, received }]
client.setPolicy(policy) / policy()
client.search(q)                              -> [SearchHit]
client.product(key)                           -> ProductView | null
client.listing(id)                            -> ListingView | null
client.authorView(id)                         -> AuthorView | null
client.record(id)                             -> signed record | null
client.stats()                                -> Stats
client.exportJsonl()                          -> string
client.onChange(callback)                     -> unsubscribe function
```

### 8.3 Node CLI

```text
perch-node serve --listen 127.0.0.1:7700 --data ./data [--peer ws://host/v1/sync]...
                 [--attestor <author hex>]... [--web ./web]
perch-node keygen
perch-node demo --out demo.jsonl     # writes a signed demo dataset
```

## 9. Build and test

- Native: `cargo test --workspace`.
- Wasm: `scripts/build-wasm.sh` builds `perch-wasm` and writes bindings to `web/pkg`.
- Test layers:
  1. Unit and property tests in each crate.
  2. Simulation: many in-memory nodes with random topologies converge to identical stores and views.
  3. Live: several `perch-node` processes on localhost sync over real WebSockets.
  4. Browser: headless Chromium runs the web app against live nodes and compares wasm view output with native API output.
  5. Adversarial: tampered records, forged signatures, forks, replayed records, sybil flags.

## 10. Decisions log

| # | Decision | Reason |
|---|---|---|
| D1 | JSON with canonical encoding instead of a binary format | The dataset must be readable and checkable by anyone with common tools. |
| D2 | SHA-256 and Ed25519 | Standardized; available in browser [Web Crypto](https://www.w3.org/TR/webcrypto/), so third parties can verify without Perch code. |
| D3 | Per-author logs instead of an unordered set | Makes deletion and edits detectable; makes sync a simple per-log catch-up. |
| D4 | Body schema checks happen in views, not at log acceptance | Old and new software agree on which records exist; schema changes do not split the network. |
| D5 | Views run on the client | Ranking and moderation cannot be secretly changed by a node operator. |
| D6 | Real-world identity attestations first | Strong sybil resistance for flags and reviews; anonymity added later via anonymous credentials. |
| D7 | Native nodes plus browser light peers over WebSocket | Browsers cannot accept inbound connections. Browser-to-browser WebRTC is a later addition. |
| D8 | Plain JavaScript UI over wasm | Keeps the UI small and auditable; all trust-relevant logic stays in Rust. |

## Appendix: terms

| Term | Meaning |
|---|---|
| Attestor | An author trusted by a policy to vouch for the real-world identity of other authors. |
| Canonical encoding | The single byte form of a record that is hashed and signed. |
| Fork | Two different records by one author at the same log position. Proof of misbehavior. |
| GTIN | Global Trade Item Number; the number behind UPC and EAN barcodes. |
| MPN | Manufacturer Part Number. |
| Root ID | ID of the first record in a chain of `replaces`. |
| Sans-IO | Code that computes protocol decisions without performing network or disk operations. |
| Sybil attack | One actor creating many identities to gain influence. |
| View | A deterministic result computed from records and a policy. |
| wasm | WebAssembly, a portable binary format that runs in browsers. |
