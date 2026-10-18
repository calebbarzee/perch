# Perch node HTTP API

API version: **v1** (path prefix `/v1`). Contract: [`ARCHITECTURE.md`](../ARCHITECTURE.md) sections 7.8, 8.1, 8.3, and 9. Implementation: [`crates/perch-node`](../crates/perch-node).

A node serves this API for people and programs that do not run the Perch client. A node is trusted for availability only (ARCHITECTURE 2.1). Every record it returns is signed, so a client can check it; every view it computes can be recomputed from the records with [`perch-views`](../crates/perch-views).

## 1. Conventions

| Topic | Rule |
|---|---|
| Base URL | `http://<host>:<port>`; the default listen address is `127.0.0.1:7700`. |
| Encoding | UTF-8. Responses are JSON (`application/json`) unless the table says JSON Lines (`application/x-ndjson`): one JSON value per line, each line ending in `\n`. |
| IDs | Record and author IDs are 64 lowercase hex characters. Uppercase is rejected. |
| Product keys | Product keys contain `:` and may contain other reserved characters. Percent-encode the key as one path segment ([RFC 3986 section 2.1](https://www.rfc-editor.org/rfc/rfc3986#section-2.1)); `/` must be sent as `%2F`. |
| View output | View endpoints return `canonical_json(view)` ([RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)), so output can be compared byte for byte with the browser client. |
| Policy | View endpoints compute with the node's default policy. `?now=<ms>` pins `policy.now`; without it, `now` is the node's current time. The `X-Perch-Policy-Hash` response header carries the policy hash (ARCHITECTURE 7.1). |
| CORS | `Access-Control-Allow-Origin: *` for `GET`, `HEAD`, and `POST`. No cookies or credentials are used. |
| Records | Records are returned in canonical wire form (ARCHITECTURE 4.2). Records without a body were removed by a node or not yet received. |

## 2. Errors

Every error has an HTTP status and this body:

```json
{ "error": { "code": "bad_id", "message": "record ID: expected 64 lowercase hex characters, got 3" } }
```

| Status | `code` | Meaning |
|---|---|---|
| 400 | `bad_id` | An ID in the path is not 64 lowercase hex characters |
| 400 | `bad_query` | A query parameter has a wrong value |
| 400 | `bad_json` | The `POST` body is not valid JSON or JSON Lines |
| 400 | `bad_encoding` | The `POST` body is not UTF-8 |
| 400 | `bad_prefix` | The lookup prefix is not 3 to 64 lowercase hex characters |
| 404 | `not_found` | No such record, author, product, listing, or path |
| 409 | `conflict` | Operator control refused (for example removing a control-kind body) |
| 413 | `too_large` | `POST` body larger than 4 MiB |
| 413 | `too_many_records` | More than 1,000 records in one `POST` |
| 429 | `rate_limited` | Too many submissions from one address |
| 429, 503 | `too_many_sessions` | Sync session limits reached |
| 500 | `internal` | Node failure, for example a disk write error |

Invalid records inside a valid `POST` body do not fail the request. Each gets its own outcome (section 4.3).

## 3. Endpoints

| Method and path | Response | Policy-dependent |
|---|---|---|
| `GET /v1/status` | Node information | no |
| `GET /v1/policy` | `{ policy, hash }` | — |
| `POST /v1/records` | Per-record outcomes | no |
| `GET /v1/records/{id}` | One record | no |
| `GET /v1/authors/{id}` | `AuthorView` | yes |
| `GET /v1/authors/{id}/log?from=&limit=` | JSON array of records | no |
| `GET /v1/search?q=&sort=&currency=&limit=` | `[SearchHit]` | yes |
| `GET /v1/products/{key}` | `ProductView` | yes |
| `GET /v1/products/{key}/records` | JSON Lines: related set | yes |
| `GET /v1/lookup/{prefix}` | JSON Lines: related sets of a hash bucket | yes |
| `GET /v1/listings/{id}` | `ListingView` | yes |
| `GET /v1/stats` | `Stats` | no |
| `GET /v1/export.jsonl` | JSON Lines: entire dataset | no |
| `GET /v1/sync` | WebSocket upgrade: sync protocol | no |
| `GET /*` | Static files of the web app (`--web`) | no |

View shapes (`AuthorView`, `ListingView`, `ProductView`, `SearchHit`, `Stats`) are defined in ARCHITECTURE 7.8 and [`docs/views.md`](views.md).

## 4. Endpoint details

### 4.1 `GET /v1/status`

```json
{
  "node": "4f0c9a1e2b3d4c5e6f708192a3b4c5d6",
  "software": "perch-node/0.1.0",
  "version": "0.1.0",
  "build_hash": "9d1e…",
  "protocol": 1,
  "listen": "127.0.0.1:7701",
  "public_url": "ws://127.0.0.1:7701/v1/sync",
  "started_at": 1760000000000,
  "uptime_ms": 5123,
  "now": 1760000005123,
  "policy_hash": "51ab…",
  "admission": "tiers",
  "store": { "records": 321, "authors": 40, "forked_authors": 1, "products": 12, "held": 0, "bodyless": 3, "removed": 0 },
  "peers": [
    { "session": 1, "direction": "outbound", "url": "ws://127.0.0.1:7702/v1/sync", "remote": null,
      "node": "77aa…", "role": "node", "state": "live",
      "messages_sent": 12, "messages_received": 14, "records_sent": 0, "records_received": 321,
      "behind": 0, "ahead": 0, "score": 0 }
  ],
  "known_addresses": 2
}
```

| Field | Meaning |
|---|---|
| `node` | Random ID of this process (changes on restart); the sync `hello.node` |
| `build_hash` | SHA-256 of the running executable file. Compare with the published release hash. |
| `store.records` | Stored log records (complete prefixes), excluding evidence-only records |
| `store.products` | Number of product keys; lets the extension choose a lookup prefix length |
| `store.held` | Out-of-order records waiting for a gap to fill |
| `store.bodyless` | Records held without a body |
| `store.removed` | IDs on the node's removal list |
| `peers[].behind` / `ahead` | Authors whose head at the peer is lower / higher than here |
| `peers[].score` | Protocol violation points of the session (docs/sync-protocol.md section 10) |

### 4.2 `GET /v1/policy`

```json
{ "policy": { "now": 1760000000000, "trusted_attestors": [ … ], "flag_threshold": 3, … }, "hash": "51ab…" }
```

The node's default policy, for information only. The official app never adopts it without the user's explicit action (ARCHITECTURE 10). `?now=` sets `policy.now` in the response.

### 4.3 `POST /v1/records`

Submit signed records. The body is either a JSON array of records or JSON Lines. At most 4 MiB and 1,000 records.

```http
POST /v1/records HTTP/1.1
Content-Type: application/x-ndjson

{"author":"1f…","body":{"name":"Ada","role":"buyer"},"body_hash":"…","kind":"profile","prev":null,"seq":1,"sig":"…","ts":1760000000000,"v":1}
{"author":"1f…","body":{…},"body_hash":"…","kind":"review","prev":"9a…","seq":2,"sig":"…","ts":1760000100000,"v":1}
```

```json
{
  "summary": { "inserted": 1, "duplicate": 0, "fork": 0, "gap": 0, "rejected": 1 },
  "results": [
    { "line": 1, "id": "9a…", "outcome": "inserted", "error": null },
    { "line": 2, "id": null, "outcome": "invalid", "error": "signature: verification failed" }
  ]
}
```

- For a JSON array, results carry `index` (0-based) instead of `line` (1-based).
- The node verifies each record, stores it, writes it to disk, and gossips it to connected peers before it responds.
- A record that cannot be stored on its own is tried as a fork evidence pair with the record before it, so a pair can be submitted as two consecutive records.

| `outcome` | Meaning | Counted as |
|---|---|---|
| `inserted` | Stored | `inserted` |
| `body_filled` | Already held without a body; the body was added | `inserted` |
| `duplicate` | Already held | `duplicate` |
| `gap` | `seq` is above the next position; held until the missing records arrive | `gap` |
| `above_fork` | The author is forked below this seq; not stored | `gap` |
| `fork` | Fork evidence; the author is now (or already was) forked | `fork` |
| `invalid` | Not isolated-valid: parse error, unknown field, size, body hash, or signature | `rejected` |
| `ts_decrease` | `ts` is lower than the previous record's `ts` | `rejected` |
| `admission` | Over the node's admission limit for this author (section 6) | `rejected` |
| `not_evidence` | Submitted as a pair but not fork evidence | `rejected` |

### 4.4 `GET /v1/records/{id}`

Returns one record: a log record, or one record of a stored evidence pair (without body). 404 if not stored.

### 4.5 `GET /v1/authors/{id}/log?from=&limit=`

A JSON array of the author's stored log records with `seq ≥ from`, in seq order. `from` defaults to 1. `limit` defaults to 100 and is at most 1,000. For a forked author, the log ends at fork seq − 1; the evidence records are available from `/v1/records/{id}` using the IDs in the sync heads or `/v1/products/{key}/records`.

### 4.6 `GET /v1/search`

| Parameter | Default | Values |
|---|---|---|
| `q` | empty | Query text |
| `sort` | `relevance` | `relevance`, `price`, `rating_count`, `rating_mean` |
| `currency` | none | ISO 4217 code preferred for `min_price` and price sorting |
| `limit` | 100 | 1 to 1,000 |
| `now` | current time | Unix ms |

### 4.7 `GET /v1/products/{key}` and `GET /v1/products/{key}/records`

`/v1/products/{key}` returns the `ProductView`. Example: `GET /v1/products/gtin%3A04006381333931`.

`/records` returns the product's **related set** as JSON Lines: every record that `ProductView` reads, including fork evidence records (header only) and restriction inputs (ARCHITECTURE 8.1). Lines are sorted by record ID. Load them into a sparse store (ARCHITECTURE 5.5) to recompute the view locally:

```js
const client = PerchClient.create({ sparse: true });
client.ingest(await (await fetch(`${node}/v1/products/${encodeURIComponent(key)}/records`)).text());
client.product(key);  // same ProductView as the node, for the same policy
```

### 4.8 `GET /v1/lookup/{prefix}`

JSON Lines: the union of the related sets of every product whose `sha256(key)` hex starts with `prefix`, sorted by record ID. The contract value is 4 hex characters; the node accepts 3 to 64. The node does not learn which product in the bucket the client wanted. `store.products` in `/v1/status` gives the catalog size for choosing a prefix length ([`docs/threat-model.md`](threat-model.md)).

### 4.9 `GET /v1/listings/{id}`, `GET /v1/authors/{id}`, `GET /v1/stats`

`ListingView` for any record ID of a listing chain, `AuthorView`, and `Stats`. 404 when the node knows nothing about the ID.

### 4.10 `GET /v1/export.jsonl`

The entire dataset as JSON Lines: for each author in ascending order, its log records by seq, then its evidence pair (2 lines). Importing this file into an empty node or client rebuilds the same state. The response is streamed from a snapshot taken when the request arrives.

### 4.11 `GET /v1/sync`

WebSocket upgrade for the sync protocol ([`docs/sync-protocol.md`](sync-protocol.md)). Text frames only; at most 4 MiB per message.

## 5. Limits

| Limit | Value |
|---|---|
| `POST /v1/records` body | 4 MiB (4,194,304 bytes) |
| `POST /v1/records` records | 1,000 |
| `POST /v1/records` rate per client address | 2 per second, burst 10; loopback exempt |
| Record line | 131,072 bytes |
| Log page | 1,000 records |
| Search hits | 1,000 |
| Sync message | 4 MiB |
| Inbound sync sessions | 256 total, 8 per address (loopback exempt) |
| Sync messages per session | 100 per second, burst 200; 16 MiB per second, burst 32 MiB |
| Sync keepalive | ping every 30 s; close after 90 s without a frame |
| Sync heads re-exchange | every 600 s (`--refresh-secs`) |

## 6. Admission

The node applies the admission tiers of ARCHITECTURE 9 with `perch_views::admission_tier`. The admission attestor set is the attestors of the node's default policy.

| Tier | Limit | Option |
|---|---|---|
| 1: verified authors and attestors | 1,000,000 records and 1 GiB per **person**: summed over all keys of the person (`perch_views::person_keys`) | `--tier1-records` |
| 2: unverified authors referenced by tier-1 records | 1,000 records and 16 MiB per author | `--tier2-records` |
| 3: everyone else | 100 records and 1 MiB per author, and 1 GiB for all tier-3 logs together | `--tier3-records`, `--tier3-budget` |

Tier-3 admission order:

- Each tier-3 author has the rank `sha256(author ‖ epoch)`, where `author` is the 32 key bytes and `epoch = floor(floor(now_ms / 86,400,000) / 30)` as 8 bytes big-endian. The order rotates every 30 days.
- When a record does not fit in the budget, the node drops whole logs of higher-ranked tier-3 authors to make room. If only lower-ranked logs remain, the record gets outcome `admission`.
- After a log is dropped or a record refused for budget, authors ranked above it are refused until the next maintenance tick. The stored tier-3 logs are therefore always the lowest ranks.
- Dropped logs are removed from `records.jsonl` at the next maintenance tick (compaction).

Other rules:

- A record over a limit gets outcome `admission`; the log keeps its complete prefix.
- Tiers are recomputed when attestations, flags, appeals, reviews, retractions, or forks arrive. Every `--refresh-secs` (default 600 s), a maintenance tick re-evaluates the tier-3 pool and its ranks, and every session re-exchanges heads, so a tier rise fetches the rest of a log.
- At startup the node replays `records.jsonl` without limits, then applies the tiers to every stored author.
- `--open-admission` admits every author in full.

## 7. Command line

```text
perch-node serve [--listen 127.0.0.1:7700] [--data ./data] [--peer ws://host:port/v1/sync]...
                 [--attestor <author hex>]... [--policy policy.json] [--web ./web]
                 [--removed ids.txt]... [--admin 127.0.0.1:7790] [--operator-key key.json]
                 [--public-url ws://host:port/v1/sync] [--open-admission]
                 [--refresh-secs 600] [--max-outbound 8] [--import file.jsonl]...
                 [--tier1-records 1000000] [--tier2-records 1000] [--tier3-records 100]
                 [--tier3-budget 1073741824]
perch-node keygen [--out key.json]
perch-node demo --out demo.jsonl [--policy-out policy.json] [--manifest-out manifest.json]
                [--size small|medium|large] [--seed 1]
```

| Option | Meaning |
|---|---|
| `--listen` | Address for HTTP and sync. Port `0` picks a free port; the node prints `perch-node listening on http://<addr> …` on stdout. |
| `--data` | Directory for `records.jsonl` (append-only persistence) and `removed.txt` (removal list) |
| `--peer` | Peer to keep connected, with reconnect backoff of 1 s doubling to 60 s, ±20% jitter |
| `--attestor` | Trusted attestor for all attestation kinds, added to the default policy |
| `--policy` | Default policy file. `now` may be omitted. |
| `--web` | Directory served at `/`. `.wasm` files are served as `application/wasm`. |
| `--removed` | Extra removal lists (one record ID per line, `#` comments) |
| `--admin` | Loopback address for operator controls (section 8). Off by default. |
| `--operator-key` | Key that signs the node's `redact` records. Only used for redaction. |
| `--public-url` | Address announced to peers. Default `ws://<listen>/v1/sync`, or none when listening on all interfaces. |
| `--import` | JSONL files inserted at startup (for example the demo dataset) |

Environment variables:

| Variable | Meaning |
|---|---|
| `RUST_LOG` | Log filter for [`tracing-subscriber`](https://docs.rs/tracing-subscriber), for example `info` or `perch_node=debug`. Logs go to stderr. |
| `PERCH_EXIT_WITH_PID` | Exit when the process with this ID ends (Linux). Test scripts use it so that nodes never outlive them. |

`keygen` prints `{ "secret": "<64 hex seed>", "author": "<64 hex>" }`. With `--out`, it writes that JSON to a new file with mode 0600 and prints only the author.

`demo` writes the deterministic demo dataset of `perch-demo`. `--policy-out` writes the default policy (without `now`) trusting the two demo attestors, with `not_after_seq` set for attestor B, which the dataset compromises after that seq.

Persistence (`records.jsonl`): every stored record is appended with one write and synced to disk before an HTTP submission returns and after each sync message that changed the store. On start, the node replays and re-verifies the file and truncates a partial last line. Detail: `crates/perch-store/src/persist.rs`.

## 8. Operator controls and notice handling

Operator controls are served only on the `--admin` address, which must be a loopback address. Reach it from the node's host, for example over SSH.

| Method and path | Body | Result |
|---|---|---|
| `POST /admin/redact` | `{ "id": "<record id>", "reason": "illegal" \| "personal_data" \| "court_order" \| "other", "note": "…"? }` | Removes the body (ARCHITECTURE 5.3) |
| `GET /admin/removed` | — | `{ "removed": [ids] }` |

`POST /admin/redact` does the following, in order:

1. Refuses control kinds (`attest.identity`, `attest.domain`, `attest.revoke`, `retract`, `retire`) with 409.
2. Reads the product key from the record's body (for the `redact` record's `product` field).
3. Drops the body, rewrites `records.jsonl` without it, and appends the ID to `<data>/removed.txt`.
4. If `--operator-key` is set, signs a `redact` record (`target`, `reason`, `note`, `product`) with that key, stores it, and gossips it.

```json
{ "id": "9a…", "body_dropped": true, "removed_list": "./data/removed.txt",
  "redact": { "author": "…", "kind": "redact", "body": { "target": "9a…", "reason": "personal_data", "product": "gtin:04006381333931" }, … } }
```

An ID can be removed before the record arrives; the node then never stores its body. The removal list is local: other nodes keep the body unless their operators remove it too, and peers can still fetch it from them.

Suggested notice-handling procedure (threat-model T45–T47):

1. Receive the notice; record the record ID, the claimed reason, and the requester.
2. Check that the ID exists (`GET /v1/records/{id}`) and that its kind is not a control kind.
3. Decide; for `personal_data` and `court_order`, follow the applicable law.
4. Call `POST /admin/redact` with the reason. Publish the `redact` record so users see that a body was removed and why.
5. Keep `removed.txt` in backups; it must be present when the node restarts.

## Appendix: terms

| Term | Meaning |
|---|---|
| CORS | Cross-Origin Resource Sharing: browser rules for calling an API from another web origin |
| Evidence pair | Two records by one author that prove a fork (ARCHITECTURE 5.2) |
| JSON Lines | One JSON value per line ([jsonlines.org](https://jsonlines.org)) |
| Policy hash | SHA-256 of the normalized policy without `now` (ARCHITECTURE 7.1) |
| Related set | The records needed to compute one product's view (ARCHITECTURE 8.1) |
| Sparse store | A store that holds records without complete log prefixes (ARCHITECTURE 5.5) |
