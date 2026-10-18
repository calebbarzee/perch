# Perch architecture

Contract version: **3** (2026-10-18). Changes from earlier versions are listed in section 12.

Perch is an open, peer-to-peer data platform about products, prices, sellers, and reviews. It does not host sales. It tells consumers what is known about a product and who said it.

This document is the contract between all Perch components. Detailed specifications live in [`docs/`](docs/). When a detail document conflicts with this file, this file wins until it is updated.

## 1. Goal: trust by construction

Users should be able to trust Perch because of how it is built, not because of who runs it. Each guarantee below is enforced by a mechanism that any user can check.

| Guarantee | Mechanism |
|---|---|
| Every claim has a known author | Each record is signed with the author's [Ed25519](https://www.rfc-editor.org/rfc/rfc8032) key. |
| No one can alter a claim | Each record is identified by the [SHA-256](https://csrc.nist.gov/pubs/fips/180-4/upd1/final) hash of its signed header, which includes the hash of its body. Any change produces a different ID and breaks the signature. |
| No one can silently edit or delete history | Each author publishes an append-only log. Each record names the ID of the previous one. Two different records at the same position prove misbehavior. |
| No one controls the data | The full dataset is a file of signed records. Anyone can copy it, run a node, or fork the software. |
| No one can buy ranking | Ratings, rankings, and moderation are computed on the user's device by published, deterministic code. Inputs are public records and a published policy only. |
| Moderation is visible | Moderation hides records according to a published policy. It never deletes them. Users can inspect and change the policy. |
| Legal removal is visible | A node may drop the body of a record for legal reasons. The signed header stays, the log stays intact, and the removal is announced with a reason. |

## 2. System overview

```
             signed records (JSON)
  ┌──────────┐   WebSocket sync   ┌──────────┐
  │ Browser  │◄──────────────────►│  Node A  │◄──┐
  │ (wasm)   │◄────────┐          └──────────┘   │ WebSocket sync
  └──────────┘         │          ┌──────────┐   │
  ┌──────────┐         └─────────►│  Node B  │◄──┘
  │ 3rd-party│  HTTP API (read)   └──────────┘
  │ client   │───────────────────────►▲
  └──────────┘
```

- **Records** are the only data. Everything else is derived from them.
- **Nodes** are native processes that store, verify, and relay records. Anyone can run one. Nodes are equal; there is no central server.
- **Browser clients** run the same Rust code compiled to WebAssembly (wasm). They verify every record and compute every view locally. They connect to several nodes over WebSocket.
- **Views** are pure functions from (records, policy) to results. Examples: merged product facts, rating summaries, price history, moderation status, search ranking.

### 2.1 Trust boundaries

| Component | Trusted for | Not trusted for |
|---|---|---|
| Records | Nothing until verified | — |
| Node | Availability only | Content, completeness, view results, served app code |
| Client code (wasm + JS) | Verification and views | — it must be checkable (below) |
| Attestors | The identity and domain claims a policy accepts from them | Anything else |

A node can serve the web app. The served code computes views and holds keys, so a malicious node could serve altered code. Mitigations:

- Builds are reproducible. Each release publishes the SHA-256 of the wasm module and of each app file.
- The app computes and displays the SHA-256 of the wasm module it runs and its own version.
- Signed static releases and the browser extension are the primary distribution. Node-served apps are a convenience.

A single node can hide records from a client (an eclipse attack). The default client connects to at least 3 nodes when available, compares heads, and shows any disagreement.

## 3. Components

| Crate / folder | Purpose | Runs on |
|---|---|---|
| `crates/perch-core` | Record types, canonical encoding, keys, signatures, receipts, isolated validation, product keys | native, wasm |
| `crates/perch-store` | `Store` trait, in-memory store with indexes, append-only JSONL persistence, log continuity and fork detection | native, wasm |
| `crates/perch-sync` | Sync protocol as a state machine with no I/O ("sans-IO"); multi-node simulation tests | native, wasm |
| `crates/perch-views` | Deterministic views and the `Policy` type | native, wasm |
| `crates/perch-demo` | Deterministic demo and test dataset generator | native, wasm |
| `crates/perch-node` | Native node binary: WebSocket sync, HTTP API, persistence, CLI, serves the web app | native |
| `crates/perch-wasm` | `wasm-bindgen` bindings: the `PerchClient` JavaScript API | wasm |
| `crates/perch-tests` | Cross-crate integration and adversarial tests | native |
| `web/` | Official web app. Plain HTML, CSS, and JavaScript modules over `PerchClient` | browser |
| `extension/` | Browser extension that shows Perch data on retailer pages | browser |
| `tests/` | Live multi-process tests and browser tests ([Playwright](https://playwright.dev)) | native, browser |

Dependency direction: `core ← store ← views`, `core ← store ← sync`, `views + sync ← node`, `views + sync ← wasm`. `perch-demo` depends on `core` only.

## 4. Records

### 4.1 Fields

A record is a JSON object with a signed **header**, a signature, and a **body**.

| Field | Part | Type | Rule |
|---|---|---|---|
| `v` | header | integer | Always `1`. |
| `author` | header | string | Ed25519 public key, 64 lowercase hex characters. |
| `seq` | header | integer | Position in the author's log. Starts at `1`. Increases by exactly 1. |
| `prev` | header | string or `null` | ID of the author's record at `seq - 1`. `null` if and only if `seq` is `1`. |
| `ts` | header | integer | Time claimed by the author, Unix milliseconds, 0 to 2^53 − 1. Not trusted. Must be ≥ the `ts` of the record at `seq - 1` (checked at log level). |
| `kind` | header | string | Record type. Matches `^[a-z][a-z0-9_.]{0,31}$`. |
| `replaces` | header | string | Optional; omitted when absent (`null` is invalid). ID of an earlier record in the same author's log (section 4.6). |
| `body_hash` | header | string | 64 lowercase hex: SHA-256 of the canonical body. |
| `sig` | signature | string | Ed25519 signature, 128 lowercase hex characters. |
| `body` | body | object | Kind-specific content (section 4.4). Optional on the wire: absent means the body was removed by a node (section 5.3). |

Unknown fields at the top level make a record invalid. All hex is lowercase; uppercase hex is invalid.

### 4.2 Canonical encoding, ID, and signature

Canonical JSON follows [RFC 8785 (JSON Canonicalization Scheme, JCS)](https://www.rfc-editor.org/rfc/rfc8785) restricted to integers:

- UTF-8, no whitespace outside strings.
- Object keys sorted by their UTF-16 code units, as in RFC 8785.
- Numbers are integers in the range ±(2^53 − 1). Input tokens with a fraction, exponent, or `-0` are invalid (`1.0`, `1e0`, `-0`).
- Strings escape only `"`, `\`, and control characters below U+0020. Use `\b \t \n \f \r` where defined, otherwise `\u00xx` with lowercase hex. Lone surrogates are invalid.

Input parsing rules:

- Duplicate keys at any depth are invalid.
- Nesting depth above 16 is invalid. The record object is depth 1; each nested object or array adds 1.
- A record line above 131,072 bytes is rejected before parsing.

Definitions:

- `body_canonical = canonical_json(body)`; at most 65,536 bytes.
- `body_hash = lowercase_hex(sha256(body_canonical))`
- `header_canonical = canonical_json(header)`, where header = the fields marked "header" in 4.1; at most 2,048 bytes.
- `id = lowercase_hex(sha256(header_canonical))`
- `sig = ed25519_sign(secret_key, "perch/record/v1\n" || header_canonical)`
- Signature verification is strict. Reject if the public key `A` or the point `R` is not a canonical encoding, if `A` or `R` has small order, or if `S ≥ L` (the group order). Accept only if `[S]B = R + [k]A` holds without cofactor multiplication, where `k = SHA-512(R || A || message) mod L`. This matches `verify_strict` in [ed25519-dalek](https://docs.rs/ed25519-dalek).

The stored and relayed form of a record is `canonical_json(header + sig + body)`, with `body` omitted when absent. It is one line of the JSONL format. Implementations normalize any valid input to this form. They keep unknown body fields: the body is stored as received JSON, never re-encoded from a typed struct.

### 4.3 Signature domains

| Prefix | Signs |
|---|---|
| `perch/record/v1\n` | Record headers |
| `perch/receipt/v1\n` | Purchase receipts (4.7) |

New signed structures must use a new prefix.

### 4.4 Validity levels

Validity has three levels. Keeping log acceptance independent of body schemas lets old and new software agree on which records exist.

| Level | Checked by | Rules | If it fails |
|---|---|---|---|
| Isolated | `perch-core` | Parsing rules, fields and types, sizes, `kind` format, `body_hash` matches the body when present, `body` is an object, signature | Record is rejected and never stored or relayed. |
| Log | `perch-store` | `seq` and `prev` continue the author's log; `ts` is not lower than the previous record's `ts` | Gap: held or requested by sync. `ts` decrease: rejected. Conflict: fork (section 5.2). |
| Semantic | `perch-views` | Body schema for the kind; reference rules (4.6); author permissions; time rules | Record stays stored and relayed. Views ignore it and can report why. |

Unknown kinds pass the isolated and log levels. Views ignore them.

### 4.5 Kinds (v1)

All text fields are UTF-8 strings. Lengths are in Unicode scalar values. `Id` means a record ID; references are resolved to root IDs (4.6). `Author` means a 64-hex public key. `Hex64` means 64 lowercase hex characters. `URL` means a string ≤ 2048 characters that the [WHATWG URL parser](https://url.spec.whatwg.org/) accepts with scheme `https`, no username or password, and a domain host (not an IP address). Hosts are compared after IDNA ToASCII and displayed in that ASCII (punycode) form. `ProductKey` is defined in 4.8. `Price` is `{ "amount": integer ≥ 0 in the currency's minor unit, "currency": ISO 4217 code }`. Optional fields are omitted when absent.

| Kind | Author | Body | Replaceable |
|---|---|---|---|
| `profile` | anyone | `name` (1–100), `role` (`seller`, `buyer`, `tester`, `crawler`, `attestor`, `node`, `other`), `url`? (URL), `about`? (≤2000) | yes |
| `attest.identity` | identity attestor | `subject` (Author), `person` (Hex64: a stable pseudonym for the human or business, the same for every key the attestor verifies for them), `level` (`person`, `business`), `country`? ([ISO 3166-1](https://www.iso.org/iso-3166-country-codes.html) alpha-2), `expires` (Unix ms) | yes; `subject` and `person` fixed |
| `attest.domain` | domain attestor | `subject` (Author), `domain` (lowercase ASCII hostname, ≤253), `expires` (Unix ms). Issued after the attestor checks `https://<domain>/.well-known/perch.json` or a DNS TXT record naming `subject`. | yes; `subject` and `domain` fixed |
| `attest.revoke` | attestor | `subject` (Author), `valid_through_seq` (integer ≥ 0). Declares the subject's key compromised after that seq. | no |
| `product` | anyone | `name` (1–300), `brand` (1–100), `gtin`? (8, 12, 13, or 14 digits with a valid check digit), `mpn`? (1–100), `category`? (≤200, lowercase, `/`-separated), `attributes`? (object, ≤64 entries, keys 1–64, values ≤500), `images`? (≤16 × `{ "url": URL, "sha256"?: Hex64 }`), `description`? (≤5000) | yes; `gtin` and `mpn` fixed |
| `listing` | seller | `product` (ProductKey), `url` (URL), `price` (Price), `availability` (`in_stock`, `out_of_stock`, `preorder`, `discontinued`), `condition` (`new`, `used`, `refurbished`), `title`? (≤300), `ships_to`? (list of alpha-2, ≤250), `location`? (`{ "country", "region"? (≤100) }`) | yes; `product` fixed |
| `observation` | anyone | `product` (ProductKey), `url` (URL), `price` (Price), `observed_at` (Unix ms, ≤ the record's `ts`), `listing`? (Id), `availability`? | no |
| `review` | anyone | `product` (ProductKey), `rating` (integer 1–5), `text` (1–10000), `title`? (≤200), `listing`? (Id), `receipt`? (Receipt, 4.7) | yes; `product` fixed |
| `flag` | anyone | `target` (Id of any record), `reason` (`fraud`, `counterfeit`, `misleading`, `spam`, `illegal`, `personal_data`, `other`), `note`? (≤2000) | yes; `target` fixed |
| `appeal` | author of the target | `target` (Id), `note` (1–2000) | yes; `target` fixed |
| `retract` | author of the target | `target` (Id). Withdraws the author's whole chain for that root ID. | no |
| `retire` | anyone | `successor`? (Author). Ends the author's log. Informational only: no verification or reputation transfers to the successor. | no |
| `redact` | node operator | `target` (Id), `reason` (`illegal`, `personal_data`, `court_order`, `other`), `note`? (≤2000). Announces that the author's node removed that record's body. | no |

Records with a non-replaceable kind cannot carry `replaces` and cannot be replaced. Records of `retract`, `retire`, `attest.revoke`, and `redact` cannot be targets of `retract`.

**Control kinds** are `attest.identity`, `attest.domain`, `attest.revoke`, `retract`, and `retire`. Their bodies hold no free text, and their meaning changes views, so nodes must not remove their bodies (5.3).

### 4.6 References, versions, and root IDs

- `replaces` must name a record in the same author's log with a lower `seq` and the same `kind`, of a replaceable kind. Otherwise the record is semantically invalid.
- A **chain** is a record plus all records that replace it, directly or transitively. Its **root ID** is the ID of the first record.
- The **latest version** of a chain is its semantically valid record with the highest `seq`.
- A replacement that changes a field marked "fixed" in 4.5 is semantically invalid.
- Every reference field (`target`, `listing`, `receipt.listing`) is resolved to the root ID of the referenced record's chain. Flags, appeals, retractions, and reviews therefore apply to the whole chain, not to one version.
- `retract` applies to the chain of its target's root and is valid only if the retracting author authored that chain.
- A retraction is final. Replacements of a retracted chain are ignored, including ones with a higher seq than the `retract`.

### 4.7 Receipts

A receipt is a detached token. It is not a log record. The seller creates it privately for the buyer; it becomes public only if the buyer cites it in a review. This keeps purchases private and prevents the seller from revoking it later.

```text
Receipt { listing: Id (root ID of the seller's listing),
          buyer: Author,
          purchased_at: Unix ms,
          nonce: Hex64 (32 random bytes),
          sig: 128 hex }
sig = ed25519_sign(seller_secret_key,
                   "perch/receipt/v1\n" || canonical_json({ listing, buyer, purchased_at, nonce }))
```

The receipt is valid when `sig` verifies (strictly) under the author of the listing chain, and the listing's root record exists below the seller's fork seq and revoke cutoff. Later retraction, replacement, or hiding of the listing does not invalidate the receipt. A seller therefore cannot strip verified-purchase status by re-listing.

### 4.8 Product keys

A `ProductKey` is a string that groups records about the same product.

| Priority | Condition | Key |
|---|---|---|
| 1 | Product has `gtin` | `gtin:` + GTIN zero-padded to 14 digits |
| 2 | Product has `mpn` | `mpn:` + brand + `:` + MPN, each with ASCII whitespace trimmed and ASCII letters lowercased |
| 3 | Otherwise | `rec:` + root ID of the product record |

[GTIN](https://www.gs1.org/standards/id-keys/gtin) check digits are validated.

## 5. Logs and storage

### 5.1 Logs

- Each author has one log. A node stores a log as a complete prefix: it holds record `n` only if it holds records `1` to `n − 1`.
- A node may decline to replicate an author (admission policy, section 9). It never stores an incomplete prefix.

### 5.2 Forks

A **fork** is proof that an author signed two different histories. Either of these is fork evidence:

1. Two isolated-valid records by the same author with the same `seq` and different IDs.
2. A record at `seq = k` whose `prev` differs from the ID of the stored record at `seq = k − 1`. The fork is then at `k − 1`.

Fork rules, chosen so every node reaches the same state regardless of arrival order:

- The **fork seq** `n` of an author is the lowest seq at which evidence exists.
- A store truncates the forked author's log to `seq = n − 1` and keeps exactly one **evidence pair**. Each pair is written as two records `(a, b)` sorted by `(seq, id)`. Pairs are ordered by `(fork seq, a.seq, a.id, b.seq, b.id)`, IDs compared as hex strings. A store keeps the lowest pair and replaces it when a lower one arrives.
- Sync advertises a forked author's head as `n − 1` with a forked marker, and relays the evidence pair. Records with `seq ≥ n` are not relayed otherwise. The marker is a hint: a node changes state only after verifying an evidence pair.
- Views ignore all of the author's records with `seq ≥ n` and mark the author forked.

### 5.3 Body removal (redaction)

- A node may drop the body of any record except control kinds (4.5), for example for illegal content, personal data, or a court order. It keeps the header and signature, so the log stays verifiable.
- The node operator should publish a `redact` record with the reason. Views show a `redact` notice only for records whose body is actually missing, and attribute the notice to its author.
- A node keeps a local list of removed IDs and refuses bodies for them.
- A node that holds a record without a body accepts the body later if a peer sends it and the hash matches, unless the ID is on its removal list.
- Views treat a record without a body as unavailable: excluded from aggregates and shown as a notice. A control-kind record without a body has no effect until its body arrives.

### 5.4 Persistence

- Persistence is an append-only file of records in canonical wire form, one per line ([JSON Lines](https://jsonlines.org)). The file is also the public export format.
- On start, a node replays and fully re-verifies the file.
- The `Store` trait hides storage. v1 has one implementation: in-memory indexes plus optional JSONL persistence. Browsers persist to IndexedDB or localStorage.

## 6. Sync

Detail: [`docs/sync-protocol.md`](docs/sync-protocol.md).

- Transport: WebSocket text frames carrying JSON messages. The state machine in `perch-sync` performs no I/O. Nodes, browsers, and tests drive it.
- Summary exchange: peers exchange **heads** (`author → (seq, id, forked)`), in pages. Each side then requests the records it lacks with `want { author, from_seq }`.
- Live gossip: after the initial exchange, newly accepted records are pushed to connected peers. Duplicates are dropped by ID.
- Fork evidence pairs are relayed like records.
- Body fetch: a peer can request missing bodies by record ID (`want_bodies`). A peer that strips bodies therefore cannot hide content from a node that has another source.
- Peer discovery: a node starts from a configured list of peers and exchanges known peer addresses (capped and diverse).
- Goal: **convergence**. Connected nodes end with the same log prefixes, the same bodies (except each node's own removal list), and the same evidence pairs, so they compute the same views.

## 7. Views

Detail: [`docs/views.md`](docs/views.md).

A view is a pure function `view(store, policy) -> result`. The same records and the same policy give the same result on every platform. Views never read the clock; `policy.now` supplies the time.

### 7.1 Policy

```json
{
  "now": 1760000000000,
  "trusted_attestors": [
    { "author": "<author hex>", "kinds": ["attest.identity", "attest.revoke"], "not_after_seq": null }
  ],
  "flag_threshold": 3,
  "flag_thresholds": { "review": 3 },
  "flags_require_verified": true,
  "seller_hidden_listing_limit": 3,
  "seller_min_distinct_flaggers": 9,
  "verified_purchase_requires_verified_buyer": true,
  "price_requires_domain_verified": true,
  "future_ts_tolerance_ms": 600000
}
```

- `trusted_attestors[].kinds`: the attestation kinds accepted from that attestor (`attest.identity`, `attest.domain`, `attest.revoke`).
- `trusted_attestors[].not_after_seq`: if set, only that attestor's records with `seq ≤ not_after_seq` count. Used to cut off a compromised attestor.
- `flag_thresholds` overrides `flag_threshold` per target kind.
- The **policy hash** is `lowercase_hex(sha256(canonical_json(normalized policy)))`. Normalization removes `now`, removes fields whose value is `null`, sorts `trusted_attestors` by `author`, and sorts each `kinds` list. Apps and APIs display the hash.
- The official app ships a default policy inside its signed release. Forks and users may change it. The app never adopts a node's policy silently (section 10).

### 7.2 Time

- Every time-dependent rule is evaluated at `policy.now`.
- Records with `ts > now + future_ts_tolerance_ms` are treated as not yet existing.

### 7.3 Identity and verification

- An attestation counts when: its author is a trusted attestor for its kind; it is within `not_after_seq`; it is below the attestor's fork seq and revoke cutoff; it is the latest version of its chain; it is not retracted; and `expires > now`. Hidden status never affects counting (7.6).
- An author is **verified** when at least one counting `attest.identity` names them as `subject`.
- An author's **person key** is `(attestor, person)` from their counting attestation with the lowest `(attestor, person)` pair. Views count verified flags and reviews once per person key, so one person with many keys counts once per attestor.
- `attest.revoke` counts under the same conditions as an attestation (without `expires`), and only if the same attestor has issued an `attest.identity` or `attest.domain` for that subject. A counting revoke makes views ignore the subject's records with `seq > valid_through_seq` and mark the author revoked.
- **Per-person status.** Restriction (7.6) is computed per person key and applies to every key with that person key. A person with any forked key is restricted on all keys, unless that key has a counting `attest.revoke`: a stolen key can be used to fork it, and the attestor decides theft claims. Fork truncation still applies to that key. Revocation stays per key, because it reports a stolen key, not misbehavior. This prevents a restricted or forked seller from starting over with a new key.
- `retire`: views ignore the author's records with a higher seq and mark the author retired.
- Real-world identity is the v1 basis for sybil resistance. Anonymous buyers are a planned extension (see [`docs/identity.md`](docs/identity.md)).

### 7.4 Domain verification

A listing is **domain verified** when its `url` host equals, or is a subdomain of, the `domain` of a counting `attest.domain` whose `subject` is the seller. If `price_requires_domain_verified` is true, only domain-verified listings feed price ranges and `min_price`.

### 7.5 Verified purchase

A review is a **verified purchase** when all of these hold:

1. it carries a `receipt` whose signature verifies under the author of the receipt's listing chain;
2. the receipt's `buyer` is the review's author;
3. the receipt's listing has `product` equal to the review's `product`;
4. the listing's seller is verified;
5. the review's author is verified, if `verified_purchase_requires_verified_buyer` is true;
6. the review's author is not the listing's seller.

Receipt validity follows 4.7: retracting, replacing, or hiding the listing does not remove verified-purchase status.

Only the latest review per (author, product) counts. Reviews by a product's sellers do not count in rating aggregates. Other proofs (for example [DKIM](https://www.rfc-editor.org/rfc/rfc6376)-signed receipt emails) are researched in [`docs/verified-purchase.md`](docs/verified-purchase.md). A browser extension cannot prove a purchase by itself, because users control the code it runs.

### 7.6 Moderation

- A record chain of kind `listing`, `review`, `product`, or `profile` is **hidden** when distinct, unretracted flags on its root reach the threshold for its kind. If `flags_require_verified` is true, only flags from verified authors count, once per person key. Flags by the target's author never count.
- Hiding affects display only, never semantics. A hidden record still counts for every rule that is not display: a hidden flag still counts, a hidden profile's author is still verified, and a hidden listing still anchors receipts. Flags on other kinds are recorded but hide nothing in v1. This also prevents cycles of flags hiding flags.
- An **appeal** is a public request by the target's author. It does not change status by itself. Status changes when flaggers retract their flags. Detail and alternatives: [`docs/moderation.md`](docs/moderation.md).
- A seller is **restricted** when both hold, counted per person key across all of the person's keys (7.3): hidden listings ≥ `seller_hidden_listing_limit`; and distinct counted flaggers across those listings ≥ `seller_min_distinct_flaggers`. No ratio is used, because a seller could dilute it with dummy listings. Views mark all of a restricted seller's listings.
- Forked, revoked, and retired authors per 7.3 and 5.2.
- Hidden records remain in the data. The app shows that something is hidden and why.

### 7.7 Numbers

- A `mean` is computed as an exact integer ratio, rounded half-to-even to 2 decimal places, and emitted as a JSON number (for example `4.25`).
- A median of an even count is the lower of the two middle values.
- Every sort has a final tie-break by ID or key, ascending.

### 7.8 View result shapes (JSON)

These shapes are the contract between `perch-views`, `perch-wasm`, the HTTP API, and the web app. Fields may be added. Fields may not be renamed without updating all consumers.

```text
Source       { author, record, verified: bool }
Fact<T>      { value: T, sources: [Source], conflicts: [{ value: T, sources: [Source] }] }

AuthorView   { id, name|null, role|null, url|null, about|null,
               verified: { level, country|null, attestor, expires }|null,
               status: "ok"|"forked"|"retired"|"revoked", restricted: bool,
               record_count, hidden_listing_count }

ListingView  { id (root id), latest (record id), seller (author id), seller_name|null,
               seller_verified: bool, seller_restricted: bool, domain_verified: bool,
               product, title|null, url, price: { amount, currency },
               availability, condition, updated_ts,
               status: "visible"|"hidden"|"retracted"|"unavailable",
               flags: { count, counted, reasons: { reason: n } }, appealed: bool,
               price_history: [{ ts, amount, currency, source: "seller"|"observation", record }] }

ReviewView   { id (root id), latest, author, author_name|null, author_verified: bool,
               verified_purchase: bool, rating, title|null, text, ts, listing|null,
               status: "visible"|"hidden"|"retracted"|"unavailable",
               flags: { count, counted, reasons: { reason: n } } }

RatingSummary { count, mean|null, histogram: [n1, n2, n3, n4, n5],
                verified_purchase: { count, mean|null, histogram },
                verified_identity: { count, mean|null, histogram } }

ProductView  { key, name: Fact<string>, brand: Fact<string>, category: Fact<string>|null,
               description: Fact<string>|null, gtin|null, mpn|null,
               attributes: { name: Fact<string> }, images: [{ url, sha256|null, author_verified: bool }],
               listings: [ListingView], reviews: [ReviewView], ratings: RatingSummary,
               price_range: { currency: { min, max, median } } }

SearchHit    { key, name, brand, category|null, score, why: [string],
               rating_mean|null, rating_count, min_price: { amount, currency }|null }

Stats        { records, authors, products, listings, reviews, forks, invalid, redacted }
```

`mean` and `score` may be fractional numbers per 7.7. All other numbers are integers.

Ranking inputs use only sybil-resistant signals. `SearchHit.rating_mean`, `SearchHit.rating_count`, and the rating part of `score` come from `verified_identity` reviews only. `RatingSummary.count`, `mean`, and `histogram` cover all counted reviews and are shown as secondary information. Creating keys therefore cannot change ranking.

## 8. Interfaces

### 8.1 HTTP API (official public API, served by `perch-node`)

Detail: [`docs/api.md`](docs/api.md). All responses are JSON unless noted. View endpoints accept `?now=<ms>` to pin `policy.now` and return the header `X-Perch-Policy-Hash`.

| Method and path | Result |
|---|---|
| `GET /v1/status` | Node info, record counts, peers, build hash |
| `GET /v1/policy` | The node's default policy and its hash |
| `POST /v1/records` | Submit signed records (JSON array or JSONL; ≤ 4 MiB and ≤ 1,000 records). Returns per-record outcome. |
| `GET /v1/records/{id}` | One signed record |
| `GET /v1/authors/{id}` | `AuthorView` |
| `GET /v1/authors/{id}/log?from=&limit=` | Signed records of one log |
| `GET /v1/search?q=` | `[SearchHit]` |
| `GET /v1/products/{key}` | `ProductView` |
| `GET /v1/lookup/{prefix}` | `[SearchHit]` for every product whose `sha256(key)` hex starts with `prefix` (4 hex characters). Lets the extension look up a product without revealing which one. |
| `GET /v1/listings/{id}` | `ListingView` |
| `GET /v1/stats` | `Stats` |
| `GET /v1/export.jsonl` | Entire dataset as JSON Lines |
| `GET /v1/sync` (WebSocket) | Sync protocol |

Nodes never hold user secret keys. Clients sign records locally and submit them.

### 8.2 JavaScript API (`PerchClient`, from `perch-wasm`)

All methods that return data return plain JSON-compatible objects using the shapes in 7.8.

```text
PerchClient.create()                          -> PerchClient
client.generateKey()                          -> author hex
client.exportKey()                            -> backup JSON string { secret, head: { seq, id } | null }
client.importKey(backupOrSecretHex)           -> author hex
client.author()                               -> author hex | null
client.publish(kind, body, { replaces?, ts? }) -> { id, record }
client.issueReceipt(listingRootId, buyerAuthor, purchasedAt) -> Receipt (signed by the current key)
client.verifyReceipt(receipt)                 -> { valid: bool, seller: author hex | null, error: string | null }
client.ingest(recordsOrJsonl)                 -> { inserted, duplicate, fork, gap, rejected, errors: [] }
client.connect(wsUrl) / disconnect(wsUrl)     -> Promise
client.peers()                                -> [{ url, state, sent, received, behind, ahead }]
client.setPolicy(policy) / policy() / policyHash()
client.search(q)                              -> [SearchHit]
client.product(key)                           -> ProductView | null
client.listing(id)                            -> ListingView | null
client.authorView(id)                         -> AuthorView | null
client.record(id)                             -> signed record | null
client.verifyRecord(json)                     -> { valid, id, error }
client.stats()                                -> Stats
client.buildInfo()                            -> { version, wasm_sha256 }
client.exportJsonl()                          -> string
client.onChange(callback)                     -> unsubscribe function
```

`peers()[].behind` is the number of authors whose head at the peer is lower than locally; `ahead` is the reverse.

`publish` refuses to write at or below the highest known own head (from local storage, the imported backup, and every connected node).

### 8.3 Node CLI

```text
perch-node serve --listen 127.0.0.1:7700 --data ./data [--peer ws://host/v1/sync]...
                 [--attestor <author hex>]... [--web ./web]
perch-node keygen
perch-node demo --out demo.jsonl     # writes a signed demo dataset
```

## 9. Limits and admission

| Limit | Value |
|---|---|
| Record line before parsing | 131,072 bytes |
| Canonical header | 2,048 bytes |
| Canonical body | 65,536 bytes |
| JSON nesting depth | 16 |
| `POST /v1/records` | 4 MiB and 1,000 records |
| Sync message | 4 MiB |
| Fork evidence per author | 1 pair |

Each node applies a local admission policy. It may limit records per author per day, decline unverified authors, rate-limit peers, and cap held out-of-order records per peer. Admission never breaks the complete-prefix rule. Exact defaults are in [`docs/sync-protocol.md`](docs/sync-protocol.md).

## 10. Client requirements

These apply to the official web app and extension.

- Text from records is inserted with `textContent` only, never as HTML. User text is isolated for bidirectional rendering (`dir="auto"` or `<bdi>`).
- Images from unverified authors are not loaded automatically, because loading reveals the viewer's IP address to the image host.
- A client stores its own latest head locally and never publishes at or below it. With an imported key, it publishes only after syncing that author's log from every configured node (at least 2 when available) and taking the highest head. One node hiding the user's last record therefore cannot make the user fork their own log. The extension never holds a key.
- The app uses the default policy bundled in its release. It never adopts a node's `/v1/policy` or attestor list without the user's explicit action.
- The app shows the active policy hash, the app version, and the SHA-256 of the running wasm module.
- The app connects to several nodes when configured and shows head disagreements between them.
- The extension looks up products by 4-hex hash prefix (`/v1/lookup/{prefix}`), never by the full product key. Results computed by a node are labeled with that node's address, and link to the app for local verification.

## 11. Build and test

- Native: `cargo test --workspace`.
- Wasm: `scripts/build-wasm.sh` builds `perch-wasm` and writes bindings to `web/pkg`.
- All layers: `scripts/test-all.sh`.
- Test layers:
  1. Unit and property tests in each crate.
  2. Simulation: many in-memory nodes with random topologies converge to identical stores and views.
  3. Live: several `perch-node` processes on localhost sync over real WebSockets.
  4. Browser: headless Chromium runs the web app against live nodes and compares wasm view output with native API output.
  5. Adversarial: tampered records, forged signatures, forks, replayed records, sybil flags.

## 12. Decisions log

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
| D9 (v2) | Header signs `body_hash`, body travels separately | Nodes can remove unlawful content or personal data without breaking logs. |
| D10 (v2) | Forked authors are cut at the fork seq; one canonical evidence pair | Every node reaches the same state regardless of arrival order. |
| D11 (v2) | Receipts are detached tokens, not log records | Purchases stay private until the buyer reviews; sellers cannot revoke receipts or hide sales volume behind them. |
| D12 (v2) | References resolve to root IDs; key fields fixed across versions | Price updates cannot shed flags; listings cannot move reviews to another product. |
| D13 (v2) | Verified flags and reviews count once per attested person | One person with many keys cannot multiply influence. |
| D14 (v2) | Domain attestations for seller URLs | Stops phishing listings from leading price rankings. |
| D15 (v2) | Keys stay in wasm memory with explicit backup; non-extractable Web Crypto keys deferred | Web Crypto Ed25519 support is recent and would move signing out of the shared Rust code. Revisit in v2. |
| D16 (v3) | Hiding is display-only and limited to listings, reviews, products, and profiles | Moderation cannot change verification or other rules, and flags cannot hide flags. |
| D17 (v3) | Restriction is per person and has no ratio | New keys and dummy listings cannot escape it. |
| D18 (v3) | Ranking uses identity-verified reviews only | Creating keys cannot change ranking. |

### Changes in contract version 3

From the second security review: Ed25519 rule stated as an equation (4.2); nesting depth defined (4.2); WHATWG URL rules (4.5); control kinds cannot be redacted (4.5, 5.3); retraction is final (4.6); receipt validity independent of listing status (4.7, 7.5); total order of evidence pairs and forked marker as hint (5.2); `want_bodies` and body convergence (6); per-kind attestor trust, no restriction ratio, policy hash normalization (7.1); revoke conditions and per-person status (7.3); display-only hiding for four kinds (7.6); ranking from verified reviews only (7.8); key backup with head and publish guard (8.2, 10); bundled policy (10); node-labeled extension results (10).

### Changes in contract version 2

From the security review: body hashing and redaction (4.1, 5.3); JCS key order by UTF-16 (4.2); strict parsing and verification (4.2); signature domain registry (4.3); `ts` non-decreasing (4.4); new kinds `attest.domain`, `attest.revoke`, `redact`; `person` in `attest.identity`; `receipt` kind replaced by detached receipts (4.7); https-only URLs; root-ID references and fixed fields (4.6); deterministic fork handling (5.2); policy fields and policy hash (7.1); time rules (7.2); stricter verified purchase (7.5); moderation of any kind and stricter seller restriction (7.6); number rules (7.7); new view fields (`domain_verified`, review `status` and `flags`, `redacted`, image `author_verified`); API additions (`/v1/policy`, `/v1/lookup`, `?now=`); client requirements (10); limits (9).

## Appendix: terms

| Term | Meaning |
|---|---|
| Attestor | An author trusted by a policy to vouch for the real-world identity or domain of other authors. |
| Canonical encoding | The single byte form of data that is hashed and signed. |
| Chain | A record plus all records that replace it. |
| Eclipse attack | A node isolating a client by showing it only selected data. |
| Fork | Proof that one author signed two different histories. |
| GTIN | Global Trade Item Number; the number behind UPC and EAN barcodes. |
| JCS | JSON Canonicalization Scheme, RFC 8785. |
| MPN | Manufacturer Part Number. |
| Person key | The (attestor, person pseudonym) pair used to count one human or business once. |
| Policy hash | SHA-256 of a policy without its `now` field. |
| Redaction | A node removing a record's body while keeping its signed header. |
| Root ID | ID of the first record in a chain. |
| Sans-IO | Code that computes protocol decisions without performing network or disk operations. |
| Sybil attack | One actor creating many identities to gain influence. |
| View | A deterministic result computed from records and a policy. |
| wasm | WebAssembly, a portable binary format that runs in browsers. |
