# Perch sync protocol

Protocol version: **1**. Contract: [`ARCHITECTURE.md`](../ARCHITECTURE.md) sections 5, 6, and 9. Implementation: [`crates/perch-sync`](../crates/perch-sync).

This document defines how two Perch peers exchange signed records so that connected peers end with the same data.

## 1. Scope and goals

| Goal | How |
|---|---|
| Connected peers converge | Each peer requests every log suffix, evidence pair, and body the other has and it lacks (section 11). |
| No peer is trusted | Every record is verified on arrival. A peer can withhold data but cannot alter it. |
| Bounded cost per peer | Hard limits on message size, batch size, and requests in flight (section 9). |
| Same code everywhere | The protocol is a state machine with no I/O. Nodes, browsers, and tests run the same Rust code. |

Out of scope for v1: partial replication, binary encoding, and browser-to-browser connections (section 13.3).

## 2. Transport

- One [WebSocket](https://www.rfc-editor.org/rfc/rfc6455) connection per session, at `GET /v1/sync` on a node.
- Every message is one UTF-8 text frame holding one JSON object.
- Every object has a `type` field. Unknown fields are ignored, so later minor additions do not break older peers.
- Messages on a session are processed in arrival order. Responses are sent in request order.
- Keepalive uses WebSocket ping and pong frames. A node pings every 30 s and closes a session after 90 s without any frame. Browsers answer pings automatically.
- Browsers on `https://` pages must use `wss://`.

## 3. Messages

### 3.1 Summary

| `type` | Direction | Purpose | Answered by |
|---|---|---|---|
| `hello` | both, first | Version and limits | — |
| `get_heads` | request | Ask for one page of heads | `heads` |
| `heads` | response | One page of `author → (seq, id, forked)` | — |
| `want` | request | Ask for log suffixes | `records` |
| `records` | response | Records answering one `want` | — |
| `push` | unsolicited | Newly accepted records, body fills, and new evidence pairs (live gossip) | — |
| `want_evidence` | request | Ask for fork evidence pairs | `evidence` |
| `evidence` | response | Fork evidence pairs | — |
| `want_bodies` | request | Ask for missing bodies by record ID | `bodies` |
| `bodies` | response | Full records with bodies | — |
| `get_peers` | request | Ask for peer addresses | `peers` |
| `peers` | response | Peer addresses | — |
| `error` | both | Report a problem; may close | — |

Every request is answered by exactly one response message, in request order. Unsolicited data uses only `push`. A receiver therefore matches each response to its oldest open request of that type.

In the examples below, `A…` stands for a 64-character author key, `I…` for a 64-character record ID, and `{record}` for a record in its canonical wire form (ARCHITECTURE 4.2).

### 3.2 `hello`

Each side sends `hello` as its first message, without waiting for the other.

```json
{
  "type": "hello",
  "protocol": "perch-sync",
  "versions": [1],
  "node": "9f2c51d07a4be3c1d8e6f0a2b4c6d8e0",
  "role": "node",
  "software": "perch-node/0.1.0",
  "listen": "wss://perch.example.org/v1/sync",
  "limits": { "max_message_bytes": 4194304, "max_records": 500, "max_heads": 4096 }
}
```

| Field | Type | Rule |
|---|---|---|
| `protocol` | string | Always `perch-sync`. Anything else: close. |
| `versions` | array of integers | Protocol versions this peer speaks. |
| `node` | string | 32 hex characters, random per process start. Used only to detect a connection to oneself or a second connection to the same process. Not an identity. |
| `role` | string | `node` (can accept connections) or `browser` (cannot). |
| `software` | string | Informational, ≤ 64 characters. |
| `listen` | string or `null` | The URL others can dial, or `null`. Browsers send `null`. |
| `limits` | object | The sender's receive limits. Each side sends no more than the smaller of its own and the peer's limit. |

### 3.3 Heads exchange

A **head** is the last record a peer holds for an author.

```json
{ "type": "get_heads", "after": null, "limit": 4096 }
```

```json
{
  "type": "heads",
  "heads": [
    { "author": "A1…", "seq": 12, "id": "I12…" },
    { "author": "A2…", "seq": 3,  "id": "I3…", "forked": true, "evidence": ["Ia…", "Ib…"] },
    { "author": "A3…", "seq": 0,  "id": null, "forked": true, "evidence": ["Ic…", "Id…"] }
  ],
  "next": "A3…"
}
```

| Field | Rule |
|---|---|
| `after` | Return authors strictly greater than this key (byte order of the hex string). `null` starts at the beginning. |
| `limit` | At most `min(limit, 4096)` entries per page. |
| `heads` | Sorted by `author`, ascending. |
| `seq`, `id` | The highest stored record of the log prefix. For a forked author with fork seq `n`, this is at most `n − 1` (ARCHITECTURE 5.2). `seq: 0, id: null` when no prefix record is stored. |
| `forked`, `evidence` | Present only for forked authors. `evidence` holds the IDs of the stored evidence pair `(a, b)`, sorted by `(seq, id)`. Both are hints (section 6). |
| `next` | The last author in this page if more authors remain; `null` on the last page. |

Paging is pulled by the receiver. A receiver sends the next `get_heads` with `after = next` only after it has processed the previous page. This bounds memory on both sides.

The receiver checks that pages advance and that an exchange is finite:

- every entry of a page is after the requested `after`, and `next`, when present, is the last entry of a non-empty page; otherwise `bad_message`;
- one exchange (all pages from `after: null` to `next: null`) has at most the local author count plus 100,000 entries; otherwise `misbehavior`. A node that joins a network with more authors catches up over several exchanges.
- each new exchange replaces what the previous one said about authors the receiver does not store, and the receiver closes the session (`misbehavior`) if its state about the peer ever exceeds the local author count plus 100,000 authors, from heads or from pushed records.

Each page reflects the responder's store when the page is built. Records the responder accepts after the session starts reach the peer by push (5.2), so changes between pages are not lost.

One page of 4,096 entries is about 0.7 MB of JSON (160 bytes per unforked entry).

### 3.4 `want` and `records`

```json
{ "type": "want", "wants": [ { "author": "A1…", "from_seq": 13 }, { "author": "A7…", "from_seq": 1, "to_seq": 100 } ] }
```

```json
{ "type": "records", "records": [ {record}, {record} ], "truncated": false }
```

`to_seq` is optional. It is the last seq the requester can store: its fork seq − 1 for a forked author, or its admission limit (9.2).

Rules for the responder:

1. Process `wants` in the given order.
2. For each entry, send the stored records of that author from `from_seq` up to the **current** head or `to_seq`, whichever is lower, in ascending `seq`.
3. For a forked author, stop at fork seq − 1. Records at or after the fork seq are relayed only inside evidence pairs.
4. Stop when the next record would exceed `max_records` (500) or `max_message_bytes` (4 MiB). Set `truncated: true` if any entry was cut short.
5. Send records without a body when the body is missing or on the local removal list.
6. Send exactly one `records` message per `want`, possibly with an empty `records` array.

Rules for the requester:

- At most one `want` in flight per session. The next `want` is sent after its `records` arrives.
- At most 1,024 entries per `want`.
- After each `records`, the requester computes the remaining entries from its new local heads. If `truncated` is `false`, an author that made no progress is not available from this peer, and the requester stops asking this peer for it.

### 3.5 `push` (live gossip)

```json
{ "type": "push", "records": [ {record}, {record} ], "evidence": [ [ {record a}, {record b} ] ] }
```

- `records` holds records the sender accepted after the session started (section 5) and records whose body the sender just filled (section 7).
- `evidence` holds evidence pairs the sender just formed or lowered (section 6). It is omitted when empty.
- Records of one author appear in ascending `seq`.
- At most 500 records and 256 pairs per `push`; larger batches are split.

### 3.6 Evidence

```json
{ "type": "want_evidence", "authors": ["A2…", "A3…"] }
```

```json
{ "type": "evidence", "pairs": [ [ {record a}, {record b} ] ] }
```

- An evidence pair is two records `(a, b)` that prove a fork (ARCHITECTURE 5.2), sorted by `(seq, id)`.
- Evidence records are sent without bodies. The signature covers the header, so a pair verifies without bodies. Views ignore records at or after the fork seq, so their bodies are not needed. A pair is then at most about 4.5 KB, and 256 pairs fit in one message.
- The responder sends its stored pair for each requested author it has evidence for, and omits the others. At most 256 authors per request.
- New or replaced pairs are sent unsolicited in `push` (section 6).

### 3.7 Bodies

```json
{ "type": "want_bodies", "ids": ["I5…", "I9…"] }
```

```json
{ "type": "bodies", "records": [ {record with body} ] }
```

- `want_bodies` asks for the full records of IDs the requester holds without a body. At most 1,024 IDs per request.
- The responder includes only records it holds with a body and that are not on its removal list. It omits the rest.
- The receiver accepts a body only if its hash matches `body_hash` and the ID is not on its own removal list (ARCHITECTURE 5.3).
- Body fills are sent unsolicited in `push` (section 7).

### 3.8 Peer addresses

```json
{ "type": "get_peers" }
```

```json
{ "type": "peers", "peers": ["wss://perch.example.org/v1/sync", "ws://198.51.100.7:7700/v1/sync"] }
```

- At most 100 addresses per `peers` message.
- Each address is a `ws://` or `wss://` URL, ≤ 256 characters.
- Peers send only addresses of `role: "node"` peers that announced a `listen` URL. Browser addresses are never shared.

### 3.9 `error`

```json
{ "type": "error", "code": "rate_limited", "message": "too many messages", "fatal": false }
```

| `code` | Meaning | Fatal |
|---|---|---|
| `unsupported_version` | No common protocol version | yes |
| `bad_message` | Unparseable JSON or wrong shape for a known `type` | yes |
| `too_large` | Message over the receiver's `max_message_bytes` or over an entry limit | yes |
| `rate_limited` | Peer exceeded a rate limit; the message was dropped | no |
| `misbehavior` | Peer score reached the limit (section 10) | yes |
| `self_connection` | `hello.node` equals the receiver's own `node` | yes |
| `duplicate_connection` | Another session to the same `node` exists. Both ends keep the session dialed by the peer with the lower `node` value. | yes |
| `shutting_down` | Receiver is stopping | yes |

After sending a fatal error, the sender closes the WebSocket.

## 4. Session lifecycle

### 4.1 States

| State | Entered when | Allowed to send |
|---|---|---|
| `AwaitHello` | Connection opened; own `hello` sent | nothing else |
| `Syncing` | Peer `hello` received and a common version found | everything |
| `Live` | Last page of the peer's heads processed (`next: null`) | everything |
| `Closed` | Fatal error or transport closed | nothing |

`Live` only marks that initial comparison is done. Requests may still be in flight.

### 4.2 Version negotiation

- The session version is the highest value in both `versions` arrays.
- No common value: send `unsupported_version` and close.
- Every later message uses the session version.
- A record with an envelope version a peer does not know (`v ≠ 1` in protocol v1) is isolated-invalid for that peer. A future protocol version that carries new record versions must send them only on sessions that negotiated that protocol version. This keeps old peers from scoring new peers as misbehaving.

### 4.3 Typical sequence

```text
A                                        B
|--- hello ----------------------------->|
|<---------------------------- hello ----|
|--- get_heads {after:null} ------------>|
|<------------------ get_heads {after:null}
|<--- heads {next:"A9…"} ----------------|
|---------------- heads {next:null} ---->|
|--- get_heads {after:"A9…"} ----------->|
|<--- heads {next:null} -----------------|   A is Live
|--- want [{A1…, 13}] ------------------>|
|<--- want [{A4…, 1}] -------------------|   B is Live
|--- records [...] --------------------->|
|<--- records [...] ---------------------|
|--- get_peers ------------------------->|
|<--- peers [...] -----------------------|
|          ... later ...                 |
|<--- push [new record] -----------------|
```

### 4.4 Comparing heads

For each entry `(author, seq_p, id_p)` in a page, the receiver compares it with its own head `(seq_l, id_l)`. If the author is forked locally at fork seq `n`, the receiver first caps `seq_p` at `n − 1`, because it cannot store records at or above `n`.

| Condition | Action |
|---|---|
| `seq_p > seq_l` | Request `from_seq = seq_l + 1`. If the peer's history differs, the first received record's `prev` does not match ours: fork evidence of type 2. |
| `seq_p = seq_l`, `id_p ≠ id_l` | Request `from_seq = seq_l`. The received record conflicts with ours: fork evidence of type 1. |
| `seq_p = seq_l` after capping, and the peer's uncapped head is higher | Request `from_seq = seq_l, to_seq = seq_l`. The peer's record at our head can expose a lower fork. |
| `seq_p < seq_l`, or `seq_p = seq_l` and `id_p = id_l` | No request. If the peer lacks records, it requests them from us. |

Evidence is compared separately:

| Condition | Action |
|---|---|
| Peer entry has `forked`, and its `evidence` IDs differ from the local pair or there is no local pair | Add the author to the next `want_evidence`. |
| Local author is forked, and the peer entry has no `forked` or different `evidence` IDs | Send the local pair in a `push`. |

A forked author still gets record requests below its fork seq. Two nodes can hold the same evidence pair and different records below it; the requests above then expose the lower fork.

Authors that the peer lacks entirely appear only in our heads; the peer requests them.

The receiver also records `peer_has[author] = seq_p` (uncapped). Section 5 uses this map.

## 5. Ordering and live gossip

### 5.1 Ordering

- A store holds only complete log prefixes (ARCHITECTURE 5.1), so it accepts an author's records in ascending `seq`.
- A peer sends records in the order its store accepted them. Within one session, each author's records therefore arrive in ascending `seq`, except during initial catch-up (5.3).

### 5.2 Push rule

Each session keeps `peer_has[author]`: the highest seq the peer is known to hold. It is updated from the peer's heads, from records received from the peer, and from records sent to the peer.

When the local store accepts a record `r` (from HTTP, a local publish, or any session), every session in state `Syncing` or `Live`:

1. skips `r` if `peer_has[r.author] ≥ r.seq`, unless `r` has a body and the peer is known to lack that body (section 7);
2. otherwise adds `r` to the next `push` and sets `peer_has[r.author] = r.seq`.

Consequences:

- **Loop prevention.** A record is pushed only when the store reports it as newly inserted. A duplicate is never forwarded. Each peer therefore forwards each record at most once per session.
- **No echo.** The session that delivered `r` already has `peer_has ≥ r.seq`, so `r` is not sent back.
- **Dedupe.** A receiver drops records whose ID it already holds. Two peers that push the same record to each other at the same time cost one wasted transfer per edge.
- **Bound.** Each record crosses each connection at most twice (once per direction). Total pushes per record ≤ 2 × number of connections.

### 5.3 Gaps

A gap is a record whose `seq` is more than one above the local head. Gaps occur when a push races with initial catch-up.

- The store holds gapped records in one bounded buffer (limits in 9.2.3), and the session requests the missing range with `want` (`from_seq = local head + 1`).
- When the store accepts a record, held records of that author that now continue the log are applied in `seq` order.
- When the buffer is full, the oldest held records are dropped. Records that peers hold are not lost: the responder of the next `want` serves up to its current head.
- For a forked author with fork seq `n`, records up to seq `n` are held, because they can still form a lower pair once the gap fills. Higher records are dropped.

## 6. Fork evidence

- The store detects forks and keeps one evidence pair per author, the lowest in the total order `(fork seq, a.seq, a.id, b.seq, b.id)` (ARCHITECTURE 5.2).
- When an insert creates or replaces a pair, every session sends the new pair in a `push`, **including** the session that delivered the triggering record. That peer may hold the other branch and not know about the fork.
- A session skips this send only if the peer's known pair (from its heads or its own `evidence`) has the same two IDs.
- The `forked` marker and `evidence` IDs in heads are hints. A peer changes state only after it verifies a pair: both records are isolated-valid, have the same author, and either have the same `seq` and different IDs, or have consecutive `seq` with `b.prev ≠ a.id`.
- If a peer marks an author forked but sends no valid pair when asked, the requester adds 20 to the peer's score (section 10) and treats the entry as unforked.
- If a received pair or record forms evidence that is not lower than the stored pair, and the sender's known pair differs from the stored pair, the receiver sends its stored pair back on that session in a `push`.
- Records at or after the fork seq are relayed only inside evidence pairs.

### 6.1 Store requirement

`perch-store` implements this requirement. Convergence of evidence depends on the store checking every incoming record, including both records of a received pair, against what it holds:

1. Same `seq` as a stored record (log or pair) with a different ID: type 1.
2. Incoming `prev` differs from the stored record at `seq − 1`: type 2 at `seq − 1`.
3. Stored record at `seq + 1` has a `prev` that differs from the incoming ID: type 2 at `seq`.

The store applies these checks also when the author is already forked, for incoming records with `seq ≤ fork seq + 1`. It then keeps the lowest pair among the stored pair and all new candidates. Records with `seq ≥ fork seq + 2` cannot form a lower pair and are dropped.

With these checks, two nodes that receive the same records of a two-branch fork in any order end with the same prefix and the lowest pair. A property test in `perch-store` checks this over 200 random delivery orders with duplicates and header-only copies.

Evidence pairs also travel in JSON Lines files (export, persistence, related sets), where they are two consecutive lines. A store that cannot store a line on its own (gap, above the fork, or refused) tries it as a pair with the line before it. This recognizes a pair even when one of its records breaks the `ts` rule.

## 7. Bodies

- A record may arrive without a body: a node removed it, or a peer stripped it.
- When a session reaches `Live`, it asks the peer for every body the local store lacks and does not refuse (removal list), at most once per ID per heads round, in requests of up to 1,024 IDs.
- When a record without a body is inserted later, the IDs are asked from every other session in the same way.
- When the store fills a missing body, the node sends the full record in a `push` to every session except the one that delivered it.
- Each session tracks which IDs the peer lacks a body for: IDs the peer sent without a body, and IDs the peer asked for that this node could not serve. When the node later stores that body, it pushes the full record to that peer even if `peer_has` covers the seq. The set is capped at 65,536 IDs per session. A peer that already has the body drops it and does not forward it. Each node therefore forwards each body fill at most once per session.
- A node never sends a body that is on its removal list.
- Control kinds (`attest.identity`, `attest.domain`, `attest.revoke`, `retract`, `retire`) cannot be on a removal list (ARCHITECTURE 4.5). A node refuses to add them.

A peer that strips bodies therefore delays content but cannot hide it from a node that has another path to a peer with the body.

## 8. Peer discovery

### 8.1 Address table

| Rule | Default |
|---|---|
| Table size | 1,000 addresses |
| Addresses per network group | 16 |
| Addresses accepted per `peers` message | 100 |
| Addresses accepted per session | 250 |
| Outbound connections | 8, including configured peers |
| Outbound connections per network group | 2 |

A **network group** is the IPv4 `/16`, the IPv6 `/32`, or for a DNS name the last two labels (for example `example.org`). Caps per group keep one operator from filling the table or all outbound slots. This defends against the address-table eclipse attacks described by [Heilman et al. (2015)](https://www.usenix.org/conference/usenixsecurity15/technical-sessions/presentation/heilman).

### 8.2 Rules

- Configured peers (`--peer`) are always dialed and never evicted.
- A node sends `get_peers` once per session after `Live`, and answers `get_peers` with up to 100 addresses, chosen at random with at most 2 per network group.
- Addresses learned from peers are untrusted. They are dialed only to fill free outbound slots, and are removed after 3 failed attempts.
- A node does not dial addresses in private, loopback, link-local, multicast, or reserved ranges learned from public peers. IPv4-mapped (`::ffff:a.b.c.d`), IPv4-compatible, and NAT64 (`64:ff9b::/96`) IPv6 addresses are judged by their IPv4 part.
- For a learned DNS name, the dialer resolves the name, refuses if any result is non-public, and connects to a checked address, so a second lookup cannot redirect it (DNS rebinding).
- Reconnect after a closed session uses exponential backoff: 1 s, doubling to at most 60 s, with ±20% random jitter.

## 9. Limits, back-pressure, and admission

### 9.1 Protocol limits

| Limit | Value |
|---|---|
| Message size | 4 MiB (4,194,304 bytes) |
| Records per `records`, `push`, or `bodies` | 500 |
| Evidence pairs per `push` | 256 |
| Heads per page | 4,096 |
| Entries per `want` | 1,024 |
| IDs per `want_bodies` | 1,024 |
| Authors per `want_evidence` | 256 |
| Pairs per `evidence` | 256 |
| Addresses per `peers` | 100 |
| Requests in flight per session, per request type | 1 |
| `push` batching | Records accepted while handling one inbound message or one local submission go into one `push` per session |

Back-pressure comes from pulls: every bulk transfer (`heads` pages and `records`) is sent only in answer to a request, and a requester has at most one request of each type in flight. A responder handles messages in arrival order and builds each response from its store when it handles the request, so it holds at most one response per session. WebSocket over TCP slows a sender whose peer reads slowly.

### 9.2 Admission

Admission is a node's choice of which authors and how much of each log it stores (ARCHITECTURE 9). It never breaks the complete-prefix rule: a node stores the first `k` records of a log, for some `k`, or none.

#### 9.2.1 Tiers

The tier of an author is computed by `perch-views` from the records and the node's admission attestor set (default: the attestors in the default policy).

| Tier | Authors | Default limit per author | Converges between honest nodes |
|---|---|---|---|
| 1 | Verified authors and attestors | 1,000,000 records and 1 GiB, summed over all keys with the same person key | yes |
| 2 | Unverified authors referenced by tier-1 records | 1,000 records and 16 MiB | yes, with the same admission attestor set |
| 3 | All others | 100 records and 1 MiB, within a node budget of 1 GiB | no; best effort |

- Tier 3 authors are admitted in ascending `sha256(author ‖ floor(day / 30))` order until the node budget is reached. `author` is the 32 key bytes; `floor(day / 30)` is 8 bytes big-endian; `day` is the node's current Unix day, so the order rotates every 30 days. The node drops whole logs of higher ranks to admit a lower rank, and after a drop or refusal it refuses higher ranks until its next maintenance tick, so it always holds a prefix of the order (`perch-store` pool, `docs/api.md` section 6).
- A limit counts canonical wire bytes of stored records.
- When an author's tier rises (for example, an attestation arrives), the node requests the rest of the log on its next heads exchange (9.2.3).
- When an author leaves tier 3 admission (budget or rotation), the node drops the whole log and its evidence pair. The node does not advertise the author afterwards.

`perch-node` computes tiers with `perch_views::admission_tier` and person keys with `perch_views::person_keys`.

#### 9.2.2 Effect on sync

| Point | Rule |
|---|---|
| Heads | A node advertises only authors it stores. |
| Requests | A requester does not ask beyond its own limit for an author: `from_seq` ≤ limit, and the author is skipped when the local log is at the limit. |
| Refusal | A record over the limit is refused with outcome `admission`. It adds no score points, because peers may use different admission sets. The requester stops asking this peer for that author until the next heads exchange. |
| Evidence | Evidence pairs are accepted for every admitted author regardless of the record limit. A pair is two records and replaces the records above the fork seq. |
| Convergence | Section 11 holds for tiers 1 and 2. Tier-3 logs may differ between nodes. They affect only the all-reviews `RatingSummary`, which the app labels as best effort (ARCHITECTURE 9). |

#### 9.2.3 Session and rate limits

| ID | Rule | Default |
|---|---|---|
| L1 | Held out-of-order records (5.3), for the whole store | 4,096 records and 32 MiB; 256 per author |
| L2 | Inbound messages per session | 100 per second, burst 200 |
| L3 | Inbound bytes per session | 16 MiB per second, burst 32 MiB |
| L4 | Inbound sessions | 256 total, 8 per IP address |
| L5 | Address table | section 8.1 |
| L6 | `POST /v1/records` per client IP address | 2 per second, burst 10 |
| L7 | Heads re-exchange on every open session | every 10 minutes (`--refresh-secs`); tier changes take effect at the next one |

- L2, L3, L6, and L7 are enforced by the I/O driver, not the state machine, because the state machine has no clock.
- A message dropped by L2 or L3 gets a non-fatal `rate_limited` error. A peer above the limit for 10 consecutive seconds is disconnected.
- L7 also repairs any state that a lost message left unsynced, at a cost of 160 bytes per author per session.

## 10. Invalid records and peer scoring

An honest peer verifies every record before storing or relaying it, so it never sends an isolated-invalid record. Each session keeps a score that starts at 0.

| Event | Points |
|---|---|
| Unparseable JSON, wrong shape for a known `type`, or an entry limit exceeded | 100 |
| Isolated-invalid record (bad signature, body hash mismatch, bad fields) | 50 |
| Record refused at log level for a reason other than a gap or fork (for example a `ts` decrease) | 10 |
| `forked` hint without a valid pair on request | 20 |
| A response (`heads`, `records`, `evidence`, `bodies`, `peers`) without an open request of that type | 10 |
| Unknown message `type` | 1 |

- At 100 points, the session sends `misbehavior` and closes. The node refuses new sessions from that IP address for 10 minutes.
- Gaps, forks, and duplicates add no points. They occur between honest peers.
- Invalid records are never stored, held, or relayed.
- A record refused by admission (9.2) adds no points, because peers may use different admission sets.

## 11. Why connected peers converge

### 11.1 State and progress

Per author, a peer's state is an evidence pair (or none), a log prefix, and the set of bodies it holds. Every change a store makes is one of:

1. a new pair, or a pair replaced by a lower one in the order of ARCHITECTURE 5.2 (the prefix may be truncated);
2. the prefix extended by one record, with the pair unchanged;
3. a body added.

Call each such change **progress**. Pairs only decrease, prefixes only grow between pair changes, and bodies are only added. Once publishing stops, only finitely many records exist, so each peer makes finitely many changes.

Two peers with the same head `(seq, id)`, the same pair IDs, and the same bodies hold the same state for that author: records are hash-linked through `prev`, so the head ID determines the whole prefix.

### 11.2 Every difference between neighbors causes progress

| Difference between neighbors P and Q | Mechanism | Why it causes progress |
|---|---|---|
| Q's head is higher than P's (after capping at P's fork seq − 1) | P requests the suffix (4.4). | Each record either extends P's prefix or conflicts with it, which creates or lowers P's pair (6.1). |
| Same head seq, different IDs | P requests from that seq (4.4). | Type 1 evidence at P. |
| Q is forked at `n`, P holds records at or above `n` | Q sends its pair (4.4). | P holds a different record at `n` or `n + 1` than the pair, so P detects a fork at `n` or lower. |
| Both forked, different pairs `X` (at P) and `Y` (at Q) | Both send their pairs (4.4, 6). | Each pair is self-contained proof. If P kept `X` after seeing `Y` and Q kept `Y` after seeing `X`, then `X ≤ Y` and `Y ≤ X`, so `X = Y`. |
| Q has a body P lacks and does not remove | P sends `want_bodies` (7). | P adds the body. |

Differences that arise after the initial exchange are covered by the pushes in sections 5.2, 6, and 7, which send every prefix extension, pair change, and body fill to every session.

### 11.3 Argument

Assumptions: the honest peers form a connected graph; every closed session is eventually re-established; messages on a session are eventually delivered; peers use the same admission attestor set, and the argument covers authors in tiers 1 and 2 (9.2); stores meet the requirement of 6.1.

1. Each peer makes finitely many changes (11.1). So there is a time after which no peer changes.
2. After that time, and once all messages in flight are delivered, no row of 11.2 applies to any pair of neighbors. Otherwise the messages of that row would cause progress.
3. So all neighbors hold the same state (11.1, last paragraph).
4. In a connected graph, equal state between all neighbors means equal state between all peers.

This is the standard argument for anti-entropy ([Demers et al. 1987](https://none.cs.umass.edu/~shenoy/courses/fall04/readings/demers.pdf)) and for state-based replicated data types ([Shapiro et al. 2011](https://inria.hal.science/inria-00609399v1)): state that only moves in one direction, plus exchange of differences between neighbors.

Limits of the argument:

- Bodies converge only within each connected group of peers that do not remove that ID. A node that removes a body neither stores nor forwards it.
- Malicious peers can delay convergence by withholding data. They cannot block it while an honest path exists, because every record and pair is self-verifying.
- A store keeps only one pair and discards other conflicting records (6.1). The final pair is the lowest pair that any node formed, so all nodes still end with the same pair. Whether that is the lowest pair of all records ever signed can depend on arrival order when an author signs three or more conflicting records near one seq. The author is marked forked on every node in either case.

## 12. Browsers

Browsers cannot accept inbound connections ([ARCHITECTURE D7](../ARCHITECTURE.md#12-decisions-log)). They participate as follows:

- A browser dials `GET /v1/sync` on one or more nodes and runs the same `Session` code compiled to wasm.
- It sends `role: "browser"` and `listen: null`. Nodes never share browser addresses.
- In v1 a browser holds a full replica. It requests every log it lacks, like a node.
- Records the user publishes are pushed to every connected node. While offline, they stay in local storage; on the next connection, the node sees them in the browser's heads and requests them.
- A browser relays records between its sessions under the same push rule. A browser connected to two nodes in different network partitions therefore joins them.
- A browser can send `get_peers` to discover more nodes. The default client connects to at least 3 nodes when available (ARCHITECTURE 2.1).
- Each session keeps the peer's heads, so the client can report per node how many authors are `behind` or `ahead` (ARCHITECTURE 8.2) and show disagreements, which exposes a node that hides records.

## 13. Comparison with other designs

### 13.1 Other protocols

| Protocol | Data model | Summary exchange | Live updates | Relation to Perch |
|---|---|---|---|---|
| [Secure Scuttlebutt](https://ssbc.github.io/scuttlebutt-protocol-guide/) (`createHistoryStream`, [EBT](https://github.com/ssbc/ssb-ebt)) | One signed append-only log per author | Vector clock: `feed → latest seq`, per followed feed | Streams stay open (`live: true`); EBT builds spanning trees ([Leitão et al. 2007](https://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf)) to avoid duplicate payloads | Perch heads are an SSB vector clock plus the head ID, which detects forks at equal seq. Perch replicates all authors, not a social graph. |
| [Nostr](https://github.com/nostr-protocol/nips/blob/master/01.md) | Unordered set of signed events | None in NIP-01: clients query relays with filters (`REQ`, `EOSE`) | Open subscriptions | Relays do not replicate each other in NIP-01. No per-author order, so omissions are not detectable. |
| [AT Protocol sync](https://atproto.com/specs/sync) | Per-account repository: signed commit over a Merkle Search Tree | `getRepo` export; commit `rev` cursor | Firehose `subscribeRepos` from hosts to relays | Hierarchical (host → relay → app). Frame limit 5 MB. Perch is a flat mesh of equal nodes. |
| [Negentropy](https://github.com/hoytech/negentropy) ([NIP-77](https://nips.4rs.nl/nips/77), [Meyer 2023](https://arxiv.org/abs/2212.13567)) | Any set of (timestamp, ID) items | Range-based set reconciliation: recursive range fingerprints | Not covered | Cost grows with the number of differences, not set size. Candidate for heads exchange at scale (13.3). |

### 13.2 Why this design fits v1

- Per-author logs reduce "which records differ" to "which seq is higher" per author. The difference is exact and needs no fingerprints or probabilistic structures.
- Heads cost 160 bytes per author per connection, regardless of how many records each author has. At 10,000 authors that is 1.6 MB per side per connection.
- Every bulk transfer is pulled, so limits and back-pressure need no extra flow-control messages.
- Flooding with dedupe is simple to reason about and to test deterministically. Its cost (each record crosses each connection at most twice) is acceptable for the expected v1 node count (tens of nodes).

### 13.3 What to adopt later for scale

| Trigger | Change |
|---|---|
| Authors > 100,000 (heads > 16 MB per connection) | Range-based set reconciliation over the heads set, keyed by author, with fingerprints of `(author, seq, id)`. Cost then grows with the number of changed authors. |
| Frequent reconnects with no changes | A heads digest in `hello`, so in-sync peers skip the heads exchange. |
| Node count or degree high enough that duplicate pushes dominate bandwidth | Lazy push as in Plumtree or SSB EBT: send full records along a spanning tree and only IDs on other links. |
| Browsers cannot hold the full dataset | Partial replication: subscriptions by product key or author set, using the related sets of ARCHITECTURE 8.1. |
| Bandwidth cost of JSON | WebSocket `permessage-deflate` ([RFC 7692](https://www.rfc-editor.org/rfc/rfc7692)) first; a binary encoding only if needed. |
| Browser-only communities | Browser-to-browser sessions over WebRTC data channels. The message set does not change. |

## 14. Tests and measurements

`crates/perch-sync/tests/sim.rs` runs many peers in one process. Each peer has a `MemoryStore` and a `Hub`; connections are in-memory queues. Delivery is in order per connection, as over WebSocket, and in random order across connections, from a seeded generator. Every scenario ends by checking that all peers have identical heads, evidence pairs, and sets of missing bodies.

| Test | Scenario |
|---|---|
| `topologies_converge` | Line, ring, star, and random graphs of 2, 5, and 12 peers; 60 records from 6 authors published at random peers while messages flow; 5% of `push` messages duplicated |
| `fifty_nodes_random_graph` | 50 peers, random connected graph |
| `late_joiners_catch_up` | Two empty peers join after 40 records, one through the other |
| `partitions_heal` | A 10-peer ring is cut in two; both sides publish; the ring is restored |
| `equivocation_yields_evidence_everywhere` | An author signs two records at one seq and submits them to two distant peers; 5 seeds |
| `equivocation_with_branches_converges` | Two partitions each extend a different branch of one author for 5 records, then meet |
| `body_stripping_peer_cannot_hide_content` | A peer strips every body it sends; honest peers that also reach an honest source end with every body |
| `deterministic_for_a_seed` | Two runs with one seed produce identical traffic |
| `cost_per_record_20_nodes` | Measures gossip cost (below) |

Measured cost (`cargo test -p perch-sync --test sim cost_per_record -- --nocapture`): 20 peers, 27 connections (random graph), 500 records of 460 bytes each, published one at a time while messages flow.

| Metric | Value |
|---|---|
| Messages per record | 35.0 |
| Bytes per record | 17,087 |
| Bytes per record per connection, relative to record size | 1.38 |

The flooding bound of section 5.2 is 2 transfers per connection; the measured value is 1.38. The difference from 1.0 is records that two neighbors push to each other at the same time.

## Appendix: terms

| Term | Meaning |
|---|---|
| Anti-entropy | Repair by exchanging differences between pairs of replicas until none remain. |
| Back-pressure | A mechanism that slows a sender when the receiver cannot keep up. |
| EBT | Epidemic Broadcast Trees: a gossip method that sends full messages along a tree and IDs elsewhere. |
| Eclipse attack | Isolating a peer so that it sees only data chosen by the attacker. |
| Evidence pair | Two records by one author that prove a fork (ARCHITECTURE 5.2). |
| Gap | A record whose `seq` is more than one above the receiver's head for that author. |
| Gossip | Forwarding newly accepted records to connected peers. |
| Head | The highest stored record of an author, as `(seq, id)`. |
| Network group | IPv4 `/16`, IPv6 `/32`, or the last two DNS labels; used to limit addresses from one operator. |
| Progress | A store change that cannot be undone: a lower evidence pair, a longer prefix, or an added body. |
| Peer score | Points a session accumulates for protocol violations; at 100 the session is closed. |
| Removal list | A node's local list of record IDs whose bodies it refuses to store or send. |
| Sans-IO | Protocol logic that performs no network, disk, or clock operations. |
| Session | The protocol state for one WebSocket connection. |
| `wss://` | WebSocket over TLS. |
