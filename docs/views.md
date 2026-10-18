# Perch views

Status: specification for contract version 3 (2026-10-18). Owner: analyst. Implementation: `crates/perch-views`.

This document defines every view in [`ARCHITECTURE.md`](../ARCHITECTURE.md) section 7. It is precise enough for a third party to reimplement the views and get byte-identical JSON. Section numbers such as "7.6" refer to `ARCHITECTURE.md`; "§3.2" refers to this document. When this document conflicts with `ARCHITECTURE.md`, the contract wins.

## 1. Principles

1. **Inputs.** A view reads only (a) the records in a store, including fork evidence pairs, and (b) a `Policy` (§2). It reads no clock, no randomness, no network, no locale, no configuration, and no hidden list.
2. **No hidden weights.** Every constant that affects a result is listed in this document (§13 lists all of them). A policy can change the policy fields; nothing else changes results.
3. **Pure functions.** The same store contents and the same policy give the same JSON bytes on every platform (native, wasm, other languages). Insertion order does not matter.
4. **Exact arithmetic.** Views use integers and exact fractions. No floating-point arithmetic affects a result (§12).
5. **Total orders.** Every list has a total order, ending with a tie-break by ID or key in ascending byte order (7.7).
6. **Visible exclusion.** When a rule excludes a record from an aggregate, the view says so and why (`excluded_reason`, `excluded` counts, `why`).
7. **Total functions.** No input record can make a view fail. perch-views has no `expect`, `unwrap`, or unchecked indexing on record-derived data, and walks `replaces` links iteratively, so a long chain cannot overflow the stack. A property test runs every view over random record soups (`tests/soup.rs`).

## 2. Policy

```json
{
  "now": 1760000000000,
  "trusted_attestors": [
    { "author": "<64 hex>", "kinds": ["attest.domain", "attest.identity", "attest.revoke"], "not_after_seq": null }
  ],
  "flag_threshold": 3,
  "flag_thresholds": { "product": 5, "review": 5 },
  "flags_require_verified": true,
  "seller_hidden_listing_limit": 3,
  "seller_min_distinct_flaggers": 9,
  "verified_purchase_requires_verified_buyer": true,
  "price_requires_domain_verified": true,
  "future_ts_tolerance_ms": 600000
}
```

### 2.1 Fields and validation

| Field | Type | Rule |
|---|---|---|
| `now` | integer | 0 to 2^53 − 1. Unix ms. The only source of time. |
| `trusted_attestors` | list | Each entry `{ author, kinds, not_after_seq }`. `author` is 64 lowercase hex and unique in the list. `kinds` is a non-empty subset of `attest.identity`, `attest.domain`, `attest.revoke`, without duplicates. `not_after_seq` is an integer ≥ 0 or `null`; it may be omitted (means `null`). |
| `flag_threshold` | integer | ≥ 1. Threshold for kinds not in `flag_thresholds`. |
| `flag_thresholds` | object | Optional (default `{}`). Kind → integer ≥ 1. |
| `flags_require_verified` | bool | |
| `seller_hidden_listing_limit` | integer | ≥ 1 |
| `seller_min_distinct_flaggers` | integer | ≥ 0 |
| `verified_purchase_requires_verified_buyer` | bool | |
| `price_requires_domain_verified` | bool | |
| `future_ts_tolerance_ms` | integer | 0 to 2^53 − 1 |

Unknown fields make a policy invalid. A threshold of 0 is invalid because it would hide every record with no flags. Every integer, including `flag_thresholds` values and `not_after_seq`, must be at most 2^53 − 1, so the policy hash is defined and JavaScript reads the same value.

`threshold(kind)` = `flag_thresholds[kind]` if present, otherwise `flag_threshold`.

### 2.2 Default policy

`default_policy(now, attestors)` returns the policy above with:

- `trusted_attestors`: one entry per given attestor, sorted by `author`, each with all three kinds and `not_after_seq: null`;
- the other values as shown, which are the v1 defaults from 7.1. [`docs/moderation.md`](moderation.md) section 5.1 gives the reasons for the moderation values.

### 2.3 Policy hash

`policy_hash(policy)` = `lowercase_hex(sha256(canonical_json(normalized)))`, where `normalized` is the policy after the steps below. The function never fails; for a policy that breaks §2.1 (only possible when built in code) the hash is not specified.

Steps:

1. parsing and validation (§2.1), with `flag_thresholds` filled in as `{}` when absent;
2. removing `now`;
3. removing every field whose value is `null` (so `"not_after_seq": null` is removed);
4. sorting `trusted_attestors` by `author`, and each `kinds` list ascending.

`canonical_json` is [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) (JCS), as in 4.2. The hash of the default policy for the demo attestors is pinned by a test in `crates/perch-views/tests`.

## 3. Record pipeline

Every view applies the same steps to every record. A record that fails a step has no effect on any view, except where a view reports it.

### 3.1 Notation

- `r.author`, `r.seq`, `r.ts`, `r.kind`, `r.replaces`, `r.id` are header fields. `r.body` is the body or **absent**.
- Hex strings (IDs, authors, `person`) compare by bytes; for lowercase hex this equals numeric order.
- Other strings compare by UTF-8 bytes, which equals Unicode scalar value order. JavaScript's default `Array.prototype.sort` compares UTF-16 code units and gives a different order for characters above U+FFFF; reimplementations must not use it.
- `(a, b) < (c, d)` is lexicographic order of tuples.
- "Valid" means semantically valid (§4) unless stated otherwise.

### 3.2 Cutoffs and active records

For an author `a`:

| Name | Definition |
|---|---|
| `fork_seq(a)` | The fork seq from the store's evidence pair (5.2), or ∞ if none. |
| `future(r)` | `r.ts > now + future_ts_tolerance_ms` (7.2). |
| `retire_A(a)` | Lowest `seq` of a record `r` by `a` with kind `retire`, body present, valid, `r.seq < fork_seq(a)`, and not `future(r)`. ∞ if none. |
| `revoke_cut(a)` | Lowest `valid_through_seq` over counting revokes with subject `a` (§5.3). ∞ if none. |
| `retire_seq(a)` | Lowest `seq` of a `retire` record that meets the `retire_A` conditions and also has `seq ≤ revoke_cut(a)`. ∞ if none. |

A record `r` is **active** when all hold:

1. not `future(r)`;
2. `r.seq < fork_seq(r.author)`;
3. `r.seq ≤ revoke_cut(r.author)`;
4. `r.seq ≤ retire_seq(r.author)`.

A record is **pre-active** when conditions 1, 2, and `r.seq ≤ retire_A(r.author)` hold. Pre-activity ignores revokes. It is used only to decide whether a revoke counts (§5.3), so revokes do not cascade.

The `future` test applies per record. A full store has non-decreasing `ts` per log, so a future record makes every later record future too. A sparse store (5.5) does not check `ts` order; testing each record gives the same result over the same records.

Fork evidence is never future: once a store holds an evidence pair, the fork is in effect regardless of the pair's `ts`.

Views ignore records that are not active, except that `AuthorView` reports cutoffs and the ignored-record counters in §8.4 and §9.6 count them.

### 3.3 Body availability

A record with an absent body is **unavailable** (5.3).

- An unavailable record of a control kind (`attest.identity`, `attest.domain`, `attest.revoke`, `retract`, `retire`) has no effect until its body arrives.
- An unavailable record of another kind can still be a chain's latest version (§3.5). The chain then has status `unavailable`.
- Semantic validity (§4) of an unavailable record is unknown. It is neither counted as invalid nor as valid.

### 3.4 Chains and root IDs

`root(r)` follows header `replaces` links:

- If `r` has no `replaces`, `root(r) = r.id`.
- If `r.replaces` names a record `p` that is in the store, has the same author, a lower seq, and the same kind, and the kind is replaceable (4.5), then `root(r) = root(p)`.
- Otherwise `r` is invalid (`replaces:*`, §4.2) and `root(r) = r.id`.

Root resolution needs only headers, so it works for unavailable records.

A **chain** is the set of records with the same root. Chain members can branch: two records may both replace the root.

The **reference version** of a chain is its **root**, when the root's body is present and schema-valid (§4.1). Fixed fields (4.5) of every other member are compared against the root (`fixed:<field>`), and the chain's product key, target, and subject come from the root (4.6).

- If the root has no body, the chain is **unavailable**: its only candidate is the root (§3.5), and every replacement has validity `root_unavailable` (neither valid nor invalid) until the root's body arrives. A body that arrives later therefore cannot move a chain to another product or change its fixed fields: replacements that differ from the root become `fixed:<field>` invalid.
- If the root has a body that fails the schema, replacements are invalid with `replaces:root_invalid`.

A chain without a reference version has no known product key, target, or subject. It appears in a product view only through a redaction notice that names a product (§9.1).

Reference resolution: a reference field (`target`, `listing`, `receipt.listing`) holding ID `x` resolves to `root(x)` if `x` is in the store, otherwise to `x` (**dangling**). A dangling reference matches nothing until the record arrives.

### 3.5 Retraction and latest version

A `retract` record `t` **counts** when: its body is present and valid (§4); it is active; its target `x` is in the store; `x` and `t` have the same author; `t.seq > x.seq`; and `x.kind` is not `retract`, `retire`, `attest.revoke`, or `redact`. A retract whose target is not in the store is dangling, has no effect, and is not invalid.

For a chain `C`:

- `retract_seq(C)` = lowest seq of a counting retract whose target resolves to `C`'s root. ∞ if none.
- `C` is **retracted** when `retract_seq(C) < ∞`.
- A member `m` is a **candidate** when: `m` is active; `m.seq < retract_seq(C)` (a retraction is final, 4.6); either `m.body` is absent (and the kind is not a control kind) or `m` is valid; and, if the root has no body, `m` is the root.
- The **latest version** `latest(C)` is the candidate with the highest seq. A chain with no candidate has no latest version and is omitted from views.
- **Versions** of `C` are its candidates in ascending seq order.

Attestor chains also apply `not_after_seq` (§5.1).

### 3.6 Display status

Chains of kind `listing`, `review`, `product`, and `profile` have a display status, in this precedence:

| Status | Condition |
|---|---|
| `retracted` | `C` is retracted. |
| `unavailable` | `latest(C)` has no body. |
| `hidden` | `counted_flags(C) ≥ threshold(C.kind)` (§7). |
| `visible` | Otherwise. |

Hiding is display-only (7.6). It never changes verification, receipts, flag counting, or rating counts (§9.4).

## 4. Semantic validity

A record with a body passes semantic validity when it passes the schema check (§4.1) and the reference and permission rules (§4.2). Validity uses only store contents. It does not depend on the policy, so `Stats.invalid` and `invalid_records` are the same for every policy.

A record of an unknown kind is not checked. It is neither valid nor invalid; views ignore it and `Stats.unknown` counts it.

### 4.1 Schema

Body schemas are the ones in 4.5, with these details.

- Fields are checked in the order 4.5 lists them. The first failure gives the reason `schema:<field path>`, for example `schema:price.currency` or `schema:images[2].url`. Missing, wrong type, wrong length, and wrong value all give the same code.
- Body fields not listed in 4.5 are ignored. They do not make a record invalid, so new optional fields do not split old and new software (D4).
- Lengths count Unicode scalar values. In JavaScript, use `[...s].length`, not `s.length`.
- Integers must be in 0 to 2^53 − 1 unless 4.5 says otherwise.
- `URL`: [WHATWG URL](https://url.spec.whatwg.org/) parse, scheme `https`, no username or password, a domain host (not an IP address), ≤ 2048 characters. perch-core provides the parser and the host in IDNA ToASCII form.
- `Price.currency` must be in perch-core's ISO 4217 table; `Price.amount` is an integer ≥ 0.
- Country codes are two ASCII uppercase letters ([ISO 3166-1](https://www.iso.org/iso-3166-country-codes.html) alpha-2 syntax). perch-core does not check that a code is assigned.
- `gtin` check digits are validated by perch-core.
- `observation.observed_at ≤ r.ts`; otherwise `schema:observed_at`.
- `review.receipt` must be an object with exactly the fields `listing` (Hex64), `buyer` (Hex64), `purchased_at` (integer), `nonce` (Hex64), and `sig` (128 hex). Any other shape gives `schema:receipt`. Unlike the body, a receipt may not carry extra fields, because they would not be signed. A receipt whose signature fails is schema-valid; it affects only `purchase` (§9.2).
- `replaces` on a non-replaceable kind (`attest.revoke`, `observation`, `retract`, `retire`, `redact`) gives `replaces:not_replaceable`.

### 4.2 Reference and permission rules

| Reason | Rule |
|---|---|
| `replaces:not_found` | `replaces` names an ID not in the store. |
| `replaces:author` | The replaced record has another author. |
| `replaces:seq` | The replaced record's seq is not lower. |
| `replaces:kind` | The replaced record has another kind. |
| `replaces:not_replaceable` | The kind is not replaceable. |
| `replaces:root_invalid` | The chain's root has a body that fails the schema. |
| `fixed:<field>` | A fixed field (4.5) differs from the chain's root. For `product`, `brand` is fixed only when the root has `mpn`. Absent and present count as different. |
| `retract:author` | The target is in the store and has another author. |
| `retract:seq` | The target is in the store and its seq is not lower than the retract's. |
| `retract:target_kind` | The target's kind is `retract`, `retire`, `attest.revoke`, or `redact`. |
| `appeal:author` | The target is in the store, and the author of its chain root is not the appeal's author. |
| `ref:listing_kind` | `review.listing`, `observation.listing`, or `review.receipt.listing` names a record in the store that is not of kind `listing`. |

Checks run in this order: schema, `replaces:*`, root availability (`root_unavailable`, not counted as invalid), `fixed:*`, then the kind-specific rules. The first failure is the reason. `invalid_records(store)` returns `[(id, reason)]` for every invalid record, sorted by `id`.

Author permissions in 4.5 that depend on the policy (for example "identity attestor") are not validity rules. An attestation from an untrusted author is valid and does not count (§5.1). `listing` may be authored by anyone; its author is by definition the seller.

## 5. Identity

### 5.1 Counting attestations

An `attest.identity` or `attest.domain` record `c` by attestor `A` **counts** when all hold (7.3):

1. `A` is in `trusted_attestors` and the entry's `kinds` contains `c.kind`;
2. `c.seq ≤ not_after_seq(A)` (∞ when null);
3. `c` is active (§3.2), so it is below `A`'s fork seq, revoke cut, and retirement;
4. `c` is the latest version of its chain, where candidates must also satisfy rule 2, and only retracts satisfying rule 2 count;
5. its chain is not retracted;
6. `c.body.expires > now`.

Hidden status never affects counting (7.6).

### 5.2 Verified, persons, and person keys

- An author is **verified** when at least one counting `attest.identity` names it as `subject`.
- A **pair** is `(attestor, person)` from a counting `attest.identity`.
- Two keys are **linked** when they share a pair. A **person** is a connected component of linked keys (7.3). A key with no pair is its own person.
- The **person key** is the lowest pair in the component. Only equality of persons affects results; the person key itself is not output.

Implementation: union-find over counting identity attestations, grouped by pair.

### 5.3 Revocation

An `attest.revoke` record `v` by `A` with subject `s` **counts** when all hold (7.3):

1. `A` is trusted for `attest.revoke`, and `v.seq ≤ not_after_seq(A)`;
2. `v` is pre-active (§3.2), so revokes against `A` do not matter;
3. `v.body` is present and valid;
4. `A`'s log holds an `attest.identity` or `attest.domain` record with subject `s`, body present and valid, at a lower seq than `v`. Its expiry and retraction do not matter.

`revoke_cut(s)` is the lowest `valid_through_seq` among counting revokes. The **reported revoke** is the counting revoke with the lowest `(valid_through_seq, attestor, id)`.

### 5.4 Author status

Precedence (7.3): `revoked` > `forked` > `retired` > `ok`.

| Status | Condition |
|---|---|
| `revoked` | `revoke_cut(a) < ∞` |
| `forked` | `fork_seq(a) < ∞` |
| `retired` | `retire_seq(a) < ∞` |
| `ok` | Otherwise |

An author is **excluded-forked** when its status is `forked`: it has fork evidence and no counting revoke. Flags and reviews of excluded-forked authors do not count (§7.2, §9.3). A forked author with a counting revoke has status `revoked`; its records up to `valid_through_seq` that are below the fork seq count normally.

### 5.5 Domains

A counting `attest.domain` gives the author the domain `body.domain`.

A listing is **domain verified** (7.4) when `host(listing.url)` equals `d` or ends with `"." + d` for a domain `d` of its seller. `host` is the URL host after IDNA ToASCII, in lowercase, from perch-core.

`ListingView.domain` is the matching domain attestation with the longest `domain`, then the lowest `attestor`, then the lowest root ID. `null` when none matches.

## 6. Restriction

Restriction is computed per person (7.3, 7.6). For a person `P`:

- **Restriction-hidden listings**: listing chains authored by any key of `P` whose reference version is active (§3.2), where `counted_flags(C) ≥ threshold("listing")`. Retraction and body removal do not matter here. Otherwise a seller could retract hidden listings to clear the count. A listing chain without flags cannot be hidden, so an implementation may skip reading it; the related set (§14) relies on this.
- `hidden_listings(P)` = their number.
- `distinct_flaggers(P)` = number of distinct persons with a counted flag on any of those listings.
- `P` is **restricted** when either holds:
  1. `hidden_listings(P) ≥ seller_hidden_listing_limit` and `distinct_flaggers(P) ≥ seller_min_distinct_flaggers`;
  2. some key of `P` is excluded-forked (§5.4). A forked key with a counting revoke does not count (7.3, threat-model R7).

Every key of a restricted person is restricted.

## 7. Flags

### 7.1 Live flags

For a target chain with root `T`, the flags on it are `flag` chains whose reference version's `target` resolves to `T`. Fixed `target` keeps every version on the same root.

For each flag chain `F`:

- `F` is **retracted** if its chain is retracted.
- `F` is **unavailable** if `latest(F)` has no body.
- Otherwise `F` is **live**; its reason is `latest(F).body.reason`.

If one author has several live flag chains on `T`, the one whose latest version has the highest seq is used. The others are `superseded`.

### 7.2 Counting

For each used live flag, the first matching rule gives its `excluded_reason`:

| Order | Reason | Rule |
|---|---|---|
| 1 | `forked` | The flagger is excluded-forked. |
| 2 | `self` | The flagger's person equals the person of `T`'s author (7.6). |
| 3 | `product_seller` | `T` is a review, and the flagger's person equals the person of any **seller of the product**: an author of a listing chain whose reference version has the review's product and is active (7.6). Listing status does not matter. |
| 4 | `unverified` | `flags_require_verified` is true and the flagger is not verified. |
| 5 | `duplicate_person` | Another flag of the same person on `T` passed rules 1–4, and its author is lower (hex). One flag per person counts. |
| — | `null` | The flag counts. |

`counted_flags(T)` is the number of flags that count. This equals the number of distinct persons with a counted flag.

### 7.3 Output

`Flags` = `{ count, counted, reasons }`:

- `count`: number of used live flags (one per author);
- `counted`: `counted_flags(T)`;
- `reasons`: reason → number among the used live flags. Reasons with 0 are omitted.

`flags(store, policy, root)` returns `[FlagView]` for every flag chain on `T`, sorted by `(ts, record)`:

```text
FlagView { record (latest version id), author, author_name|null, author_verified,
           reason|null, note|null, counted, excluded_reason|null, ts (latest version ts) }
```

`excluded_reason` for flags is one of `retracted`, `unavailable`, `superseded`, `forked`, `self`, `product_seller`, `unverified`, `duplicate_person`. For `retracted` flags, `reason` and `note` come from the latest version before the retraction. For `unavailable` flags they are `null`.

Flags on kinds other than `listing`, `review`, `product`, and `profile` are reported but hide nothing (7.6).

### 7.4 Appeals

The appeal of a chain `C` is selected from `appeal` chains that meet three conditions: the chain's reference version has a target that resolves to `C`'s root; the chain's author is `C`'s author; and the chain is live (not retracted, latest version has a body). The one whose latest version has the highest seq is used. `Appeal = { record: latest id, note, ts: latest ts }`, or `null`.

### 7.5 Redaction notices

When `latest(C)` has no body, `redaction` is a `Redaction` object. Otherwise it is `null`.

- Notices are active `redact` records with a body that are valid and whose `target` equals `latest(C).id` exactly (5.3). There is no root resolution.
- The notice used is the first by (author verified first, `ts` ascending, `id` ascending).
- `Redaction = { record, by, reason, note|null, product|null }` from that notice, or all five `null` when no notice exists.
- `product` is the notice's optional `product` field (4.5). It is stated by the notice's author, a node operator, not by the removed record. Views use it only to place a chain that has no body at all (§9.1, §8.1).

### 7.6 Moderation object

`Moderation = { status, flags: Flags, appeal: Appeal|null, redaction: Redaction|null }` for a chain, with `status` from §3.6.

## 8. Listings

### 8.1 ListingView

For a listing chain `C` with latest version `L` and reference version `R`:

| Field | Value |
|---|---|
| `id` | Root ID |
| `latest` | `L.id` |
| `seller` | Root author |
| `seller_name` | Seller's profile name if the profile status is `visible`, else `null` (§10.1) |
| `seller_verified` | §5.2 |
| `seller_restricted` | §6 |
| `seller_status` | §5.4 |
| `domain_verified`, `domain` | §5.5, using `L.url`. `false`/`null` when `L` has no body. |
| `feeds_prices` | The listing feeds price figures (§8.3) |
| `price_excluded_reason` | The first rule of §8.3 that fails: `hidden`, `retracted`, or `unavailable` (rule 1, the display status), `availability` (rule 2), `seller_restricted` (rule 3), `domain_unverified` (rule 4); `null` when `feeds_prices` |
| `product` | `R.body.product` (fixed). For a chain without any body: the `product` of its redaction notice (§7.5). |
| `title` | `L.body.title` or `null` |
| `url`, `price`, `availability`, `condition` | From `L`; `null` when `L` has no body |
| `ships_to` | `L.body.ships_to` in record order, or `[]` |
| `location` | `{ country, region|null }` or `null` |
| `updated_ts` | `L.ts` |
| `status` | §3.6 |
| `flags` | §7.3 |
| `appealed`, `appeal` | `appeal != null`; §7.4 |
| `redaction` | §7.5 |
| `price_history` | §8.2 |

`listing_view(store, policy, id)` accepts any member ID and returns the view of its chain, or `null` if the ID is unknown, not a listing, the chain has no latest version, or the chain has neither a reference version nor a notice naming a product.

### 8.2 Price history

Entries, sorted by `(ts, source rank, record)`, where `seller` ranks before `observation`:

1. **Seller entries**: one per version of `C` with a body: `{ ts: version.ts, amount, currency, source: "seller", record: version.id }`.
2. **Observation entries**: one per `observation` record `o` meeting all of these conditions: `o` is active and valid; `o.body.listing` resolves to `C`'s root; `o.body.product` equals `R.body.product`; `o`'s author is verified. Each entry is `{ ts: o.body.observed_at, amount, currency, source: "observation", record: o.id }`.

Observations from unverified authors are excluded. Free keys could otherwise add fake reference prices ("was €99") to any listing (threat-model R2).

### 8.3 Price feed

A listing **feeds prices** when all of these hold:

1. status `visible`;
2. `L.body.availability` is `in_stock` or `preorder`;
3. the seller is not restricted (this covers excluded-forked sellers, §6);
4. domain verified, if `price_requires_domain_verified` is true.

A listing that meets rules 1–3 and fails only rule 4 counts in `excluded_unverified_domain`.

Hidden listings do not feed prices. A minimum price is a summary of one listing's content, so showing it would show the hidden offer.

### 8.4 Ordering in ProductView

`listings` are sorted by:

1. `feeds_prices` first (§8.3), then the others, so an unverified or lookalike shop never leads the list (7.8);
2. within each group, status rank: `visible` 0, `hidden` 1, `retracted` 2, `unavailable` 3;
3. `price.currency` ascending, then `price.amount` ascending (listings without a price last);
4. `id`.

## 9. Reviews and ratings

### 9.1 Reviews of a product

The reviews of product key `K` are review chains that have a latest version and either:

- a reference version with `product = K`; or
- a root without a body, and a redaction notice (§7.5) with `product = K` that targets the root. The root is then the chain's latest version (§3.5), so the notice is the one the view shows.

The same rule selects the listing chains of `K`. The second case places records whose only body was removed; it is attributed to the notice's author.

### 9.2 Verified purchase

For a review with latest version `V` (body present), `purchase` is the first matching row (7.5, 4.7):

| Order | `purchase` | Condition |
|---|---|---|
| 1 | `none` | `V.body.receipt` is absent. |
| 2 | `receipt_invalid` | `receipt.listing` is not in the store or is not a `listing` (a member ID resolves to its chain root, 4.6); or the listing root is not active (below the seller's fork seq, revoke cut, and retirement, and not future). |
| 3 | `listing_unavailable` | The listing root's body is missing in this store (removed or not yet received). The receipt cannot be checked here; the outcome changes when the body arrives (4.7). |
| 4 | `receipt_invalid` | The listing root's body fails the schema; or the signature does not verify strictly under the listing chain's author (perch-core `verify_receipt`); or `receipt.buyer ≠ V.author`. |
| 5 | `wrong_product` | The listing root has `product ≠ V.body.product`. |
| 6 | `self` | The review author's person equals the seller's person. |
| 7 | `seller_unverified` | The seller is not verified. |
| 8 | `buyer_unverified` | `verified_purchase_requires_verified_buyer` is true and the review author is not verified. |
| 9 | `verified` | Otherwise. |

`verified_purchase = (purchase == "verified")`.

The listing's retraction, replacement, hiding, or body removal does not matter (4.7). `self` compares persons, not keys, so a seller cannot issue a receipt to its own second key.

### 9.3 Counting

Each review chain gets `excluded_reason` from the first matching rule. Rules 5 and 6 are evaluated over the chains that passed rules 1–4.

| Order | Reason | Rule |
|---|---|---|
| 1 | `retracted` | The chain is retracted. |
| 2 | `unavailable` | `latest(C)` has no body. |
| 3 | `forked` | The author is excluded-forked (§5.4). |
| 4 | `seller` | The author's person equals the person of any author of a listing chain for `K` (7.5). Listing status does not matter. |
| 5 | `superseded` | The same author has another chain for `K` whose latest version has a higher seq (7.5: latest review per author and product). |
| 6 | `superseded` | Another key of the same person has a chain for `K` that passed rules 1–5 with a higher `(latest ts)`; on equal `ts`, the lower author hex wins, then the lower root ID. One review per person counts (7.3). |
| — | `null` | The review counts (`counted: true`). |

A hidden review still counts. Hiding is display-only (7.6), and moderation.md section 3.3 shows that hiding reviews from ratings would let 5 hired flaggers remove any negative rating.

### 9.4 RatingSummary

Over counted reviews, using `rating` of the latest version:

- `count`, `histogram` (`[n1, n2, n3, n4, n5]`, where `nk` = reviews with rating k), `mean` = Σ rating / count (§12.1), `null` when count = 0.
- `verified_purchase`: the same over counted reviews with `verified_purchase`.
- `verified_identity`: the same over counted reviews whose author is verified.
- `excluded`: `{ hidden, unavailable, retracted, seller, forked, revoked, superseded }`:
  - `hidden` is 0 in v1, because hidden reviews count. The field stays for shape stability.
  - `unavailable`, `retracted`, `seller`, `forked`, `superseded`: number of chains with that `excluded_reason`.
  - `revoked`: number of review chains for `K` whose author has a counting revoke and whose members with a body all have `seq > revoke_cut`. These chains have no latest version and are not listed.

`verified_identity` reviews are the only rating input to ranking (7.8, §11).

### 9.5 ReviewView

| Field | Value |
|---|---|
| `id`, `latest` | Root ID; latest version ID |
| `author`, `author_name`, `author_verified`, `author_status` | Root author; profile name if `visible`, else `null`; §5.2; §5.4 |
| `verified_purchase`, `purchase` | §9.2. When `latest(C)` has no body: `false`, `"none"`. |
| `counted`, `excluded_reason` | §9.3 |
| `rating`, `title`, `text` | From the latest version; `null` when it has no body |
| `ts` | Latest version `ts` |
| `listing` | Root of `body.listing` (dangling: the raw ID), or `null` |
| `status`, `flags`, `appeal`, `redaction` | §3.6, §7 |

`reviews` in a ProductView are sorted by `(ts descending, id ascending)`.

### 9.6 Why the arithmetic mean, and where smoothing is used

`RatingSummary.mean` is the plain arithmetic mean, because users read it as "the average of these ratings" and it is exact.

A plain mean ranks badly. One 5-star review beats 100 reviews averaging 4.9. [Evan Miller (2009)](https://www.evanmiller.org/how-not-to-sort-by-average-rating.html) shows this failure for positive/negative votes. [Miller (2014)](https://www.evanmiller.org/ranking-items-with-star-ratings.html) extends it to star ratings with a Dirichlet prior. Search therefore ranks by a smoothed mean (§11.3).

## 10. Products

### 10.1 Profiles

An author's **profile chain** is the author's `profile` chain with a latest version whose latest version has the highest seq. Its status follows §3.6.

- `AuthorView.name`, `role`, `url`, `about` come from the latest version when the status is `visible` or `hidden`, and are `null` otherwise. The app collapses hidden content, as it does for listings.
- Names shown inside other views (`seller_name`, `author_name`, `FlagView.author_name`) are non-null only when the profile status is `visible`. A hidden profile name therefore does not appear across many listings.

### 10.2 Product key of a chain

The product key of a `product` chain is computed from its reference version by perch-core's product key function (4.8). For `rec:` keys, the root ID is used. `brand` and `mpn` are fixed when `mpn` is set, so all versions share the key.

`product_view(store, policy, key)` returns `null` unless at least one product chain, listing chain, or review chain with a latest version has product key `key`.

### 10.3 Fact sources

Each product chain with key `K` and a latest version has a status (§3.6). Product chains of excluded-forked authors are not used.

- The **source chains** are the `visible` chains. Facts come from them only (7.6). When no chain is visible, every fact is empty (`name.value` and `brand.value` are `""`).

Hiding applies per product chain, never to the key as a whole (7.6). Flags on one chain never hide another author's chain, and a hidden chain's text never appears, even when another key later publishes a product record for the same key: that record is a separate claim, flagged on its own.

`ProductView.moderation.status`:

1. `visible` when any chain is `visible`;
2. `hidden` when `K` has product chains and all of them are `hidden`;
3. otherwise the first of `retracted`, `unavailable` that any chain has;
4. `visible` when `K` has no product chains. `moderation.flags` is the union over all chains of `K`: `count` = distinct flagging authors over all chains, `counted` = distinct persons with a counted flag on any chain, `reasons` over the used live flags of all chains. `moderation.appeal` is the appeal with the highest `(ts, record)` over all chains. `moderation.redaction` is the redaction of the lowest-root unavailable chain when the status is `unavailable`, else `null`.

`ProductView.excluded_sources` = `{ hidden, retracted, unavailable, forked, revoked }`: the number of product chains of `K` not used as sources, by reason (`revoked`: all members with a body have `seq > revoke_cut`). Every hidden chain counts in `hidden`.

### 10.4 Fact resolution

A `Fact<T>` for a field is computed over the source chains whose latest version has that field.

1. Group sources by value. Values are compared exactly (no case folding or Unicode normalization).
2. For each value, compute:
   - `v` = number of distinct persons among its verified source authors;
   - `p` = number of distinct persons among all its source authors (an unverified key is its own person).
3. Rank values by `(v descending, p descending, value ascending by UTF-8 bytes)`.
4. `value` = the first value. `sources` = its sources. `conflicts` = the other values in rank order, each with its sources.
5. `Source = { author, record: latest version id, verified }`. Sources are sorted by `(verified descending, author, record)`.

One verified person outranks any number of unverified keys. Unverified keys cost nothing to create ([Douceur 2002](https://www.microsoft.com/en-us/research/publication/the-sybil-attack/)). The final tie-break is alphabetical: it is transparent, and any tie-break can be targeted by someone who chooses a value.

Iterative truth-discovery methods such as [TruthFinder (Yin, Han, Yu 2007)](https://research.ibm.com/publications/truth-discovery-with-multiple-conflicting-information-providers-on-the-web) learn a trust weight for each source from agreement. They are not used, because learned weights are hard for a user to check and shift whenever unrelated records arrive.

Fields:

| Field | Value |
|---|---|
| `name`, `brand` | Fact; when no source has the field: `{ "value": "", "sources": [], "conflicts": [] }` |
| `category`, `description` | Fact or `null` |
| `attributes` | Object: attribute name → Fact, for each name in at least one source |
| `gtin` | The 14 digits after `gtin:` when the key is a GTIN key, else `null` |
| `mpn` | The first value of the Fact ranking over `mpn`, else `null` |
| `images` | Union of `images` of the source chains' latest versions, deduplicated by `(url, sha256)`; `author_verified` is true when any author of that image is verified; sorted by `(author_verified descending, url, sha256 with null first)` |

### 10.5 Price range

`price_range` is an object: currency code → `{ min, max, median, count, excluded_unverified_domain }`.

- A currency appears when at least one listing of `K` in that currency meets rules 1–3 of §8.3.
- `count` = number of feeding listings in that currency. `excluded_unverified_domain` per §8.3.
- `min`, `max`, `median` are over the latest prices of feeding listings. `median` is the lower median (7.7). All three are `null` when `count` is 0.

Prices in different currencies are never combined or converted. Views have no exchange-rate input, and an exchange rate would be an input from outside the records.

The median is reported because it is robust to outliers: one fake low price moves the minimum but not the median ([NIST/SEMATECH e-Handbook, 1.3.5.1](https://www.itl.nist.gov/div898/handbook/eda/section3/eda351.htm)).

### 10.6 ProductView

`{ key, moderation, name, brand, category, description, gtin, mpn, attributes, images, listings, reviews, ratings, price_range, excluded_sources }`, with the parts defined above.

## 11. Search

### 11.1 Tokens

`tokens(s)`: scan `s` by Unicode scalar value. Each run of characters for which Rust `char::is_alphanumeric` is true forms one token. Characters outside a run are separators. Each token is lowercased with Rust `char::to_lowercase` (Unicode default case conversion). The Unicode version is that of the Rust toolchain: Unicode 17.0.0 for Rust 1.97 (§12.4).

Query tokens are `tokens(q)`, deduplicated with the first occurrence kept, and limited to the first 16.

### 11.2 Matching and text score

Candidates are the product keys of product chains with a reference version (§10.2), plus `gtin:` keys named by a GTIN query token (below) for which a product, listing, or review chain with a latest version exists. A key without product chains therefore matches only by GTIN.

Products whose `moderation.status` is not `visible` are hits too. They come after all visible hits (§11.7) and carry `moderation_status`, so the app can show that a hidden product exists without showing it among the results.

The fields of a candidate are:

| Field | Tokens | Exact | Prefix |
|---|---|---|---|
| `gtin` | The 14-digit GTIN; matches a query token of 8–14 ASCII digits that, zero-padded to 14, equals it | 100 | — |
| `name` | Tokens of every candidate value: `name.value` and each `name.conflicts[].value` | 30 | 15 |
| `mpn` | `tokens(mpn)` | 25 | 10 |
| `brand` | Tokens of every candidate value of `brand` | 20 | 10 |
| `category` | Tokens of every candidate value of `category` | 10 | 5 |

A product has a **verified source** when at least one of its visible product chains has a verified author. Which values a field matches depends on that (7.8):

| Product | Values matched for `name`, `mpn`, `brand`, `category` |
|---|---|
| With a verified source | Values from visible chains by verified authors only. A free key therefore cannot add matches ("iphone") to the product. |
| Without a verified source | Every value from every visible chain (the displayed value and its conflicts). Unverified keys therefore cannot rename the product out of search. |

Values come from visible chains only (§10.3). The hit still shows the displayed values (the facts of §10.4).

- **Exact**: a field token equals the query token.
- **Prefix**: a field token starts with the query token and is longer, and the query token has at least 2 characters.
- Each query token scores its best match: the highest points; on equal points, the field higher in the table; exact before prefix.
- A product is a hit only if every query token matches (AND). With no query tokens, every product is a hit with text score 0.

### 11.3 Rating and offer components

From `verified_identity` reviews only (7.8):

- `n` = count, `S` = sum of ratings.
- `smoothed = (15 + S) / (n + 5)`. This is the posterior mean of a Dirichlet prior with one pseudo-rating at each star level ([Miller 2014](https://www.evanmiller.org/ranking-items-with-star-ratings.html)). With no reviews it is 3. Each real review moves it less while the count is small.
- `rating_points = round2(10 × (smoothed − 3)) = round2(10 × (S − 3n) / (n + 5))`, greater than −20 and less than +20.
- `offer_points` = 5 if `min_price` is not null, else 0.

Miller ranks by a lower credible bound, which needs a square root. Perch uses the posterior mean, which is an exact fraction, to avoid floating-point results (§12).

### 11.4 Score and `why`

`score = Σ text points + rating_points + offer_points`, an exact multiple of 0.01.

`why` lists every component in this order:

1. For each query token, in query order: `<field> <exact|prefix> "<token>" +<points>`, for example `name exact "kettle" +30`. For a GTIN match: `gtin exact "<token>" +100`.
2. `rating <smoothed> from <n> verified-identity reviews <sign><points>`, for example `rating 4.21 from 12 verified-identity reviews +12.1`. With n = 0: `rating 3 from 0 verified-identity reviews +0`.
3. `has price +5` or `no price +0`.

Numbers use the format in §12.2. `<sign>` is `+` for values ≥ 0 and `-` otherwise.

### 11.5 SearchHit

| Field | Value |
|---|---|
| `key` | Product key |
| `name`, `brand` | `name.value`, `brand.value` |
| `category` | `category.value` or `null` |
| `score`, `why` | §11.4 |
| `moderation_status` | `ProductView.moderation.status` (§10.3) |
| `verified_source` | The product has a verified source (§11.2) |
| `rating_mean`, `rating_count` | `verified_identity.mean`, `verified_identity.count` |
| `rating_smoothed` | `round2(smoothed)` |
| `verified_purchase_count`, `verified_purchase_mean` | From `ratings.verified_purchase` |
| `min_price` | §11.6 |

### 11.6 Currency for `min_price` and price sort

`search(store, policy, q, options)` takes `options = { sort, currency|null, limit }`. Defaults: `relevance`, `null`, 100. `limit` is at most 1,000.

- `min_price`: if `options.currency` is set and the product has feeding listings in it, the minimum feeding price in that currency. Otherwise the minimum in the product's currency with the most feeding listings (ties: lower code). `null` without feeding listings.
- Sort currency `C`: `options.currency` if set, otherwise the most common `min_price.currency` among all hits (ties: lower code).

### 11.7 Sort orders

Every order first groups hits, then sorts within each group:

1. products with a verified source (`verified_source: true`);
2. other products whose `moderation.status` is `visible`;
3. products whose `moderation.status` is not `visible`.

Creating keys therefore cannot move a product above any product with a verified source (7.8).

| `sort` | Order within each group |
|---|---|
| `relevance` | `score` descending, then `key` |
| `price` | Group 0: `min_price.currency = C`, by `amount` ascending. Group 1: other currencies, by `(currency, amount)`. Group 2: no price. Then `key`. |
| `rating_count` | `rating_count` descending, `rating_smoothed` descending, `key` |
| `rating_mean` | `rating_smoothed` descending, `rating_count` descending, `key` |

`rating_mean` sorts by the smoothed value, so a product with one 5-star review does not outrank a product with 100 reviews averaging 4.8. Both values are shown.

The result is the first `limit` hits after sorting. Comparisons use exact fractions, not the rounded output values.

Rating and offer points add less than 25 to a hit's text points. For `relevance`, an implementation may therefore stop scoring a group once enough hits of that group have a score at least 25 above the text points of every remaining hit in it. The result is the same. perch-views does this, and it computes the verified-purchase fields only for the returned hits, because they need receipt signature checks and do not affect any sort order.

Every component of `score` appears in `why`, which meets the transparency aim of the EU [Digital Services Act, Article 27](https://eur-lex.europa.eu/eli/reg/2022/2065/oj): explain the main ranking parameters and let users change them. A user changes them through the policy, or by sorting.

## 12. Numbers and determinism

### 12.1 Rounding

`round2(x)` for an exact fraction `x = p / q` (q > 0): let `m = |p| × 100`, `k = floor(m / q)`, `r = m − k × q`. If `2r > q`, or `2r = q` and `k` is odd, add 1 to `k`. The result is `sign(p) × k / 100`. This is round half to even (7.7).

Examples: 107 / 40 = 2.675 → 2.68; 33 / 8 = 4.125 → 4.12; 35 / 8 = 4.375 → 4.38; 10 / 3 → 3.33.

`mean = round2(Σ rating / count)`.

### 12.2 Number output

A value `k / 100` (integer `k`) is written as a JSON number:

- If `k` is a multiple of 100: as an integer (`4`, not `4.0`).
- Otherwise: in decimal with 1 or 2 fraction digits and no trailing zero (`4.2`, `4.25`, `-0.5`, `0.07`).

This equals ECMAScript `Number.prototype.toString` and RFC 8785 output for these values, because each such decimal has at most 15 significant digits for the ranges used here and so round-trips through IEEE 754 binary64. perch-views emits integers as JSON integers. It emits other values as `(k as f64) / 100.0`. IEEE division is correctly rounded on every platform, including wasm, so the result is the nearest binary64. All other numbers are integers.

### 12.3 Canonical output

The canonical form of a view result is its RFC 8785 serialization. `perch_views::canonical_json(&value)` returns it. Object keys are therefore sorted by UTF-16 code units. A `BTreeMap<String, _>` sorts by UTF-8 bytes, and the two orders differ only for keys with characters above U+FFFF. Byte-identity tests compare canonical forms.

### 12.4 Hazards and how perch-views avoids them

| Hazard | Effect | Rule |
|---|---|---|
| Floating-point arithmetic | Different rounding of sums or of `x87` intermediates; [wasm NaN bit patterns are nondeterministic](https://www.webassembly.org/docs/nondeterminism/) | All arithmetic in integers and exact fractions (`u128` numerators). Only the final output conversion uses one correctly rounded division. |
| Hash map iteration | `std::collections::HashMap` uses a random seed per process | Results are built from `BTreeMap` or sorted vectors. Hash maps are used only for lookups. |
| Unstable sorts | Equal keys in different orders | Every sort has a total order ending in ID or key. |
| String order | JavaScript sorts by UTF-16 code units | Byte order for strings; RFC 8785 for object keys. |
| Unicode tables | `is_alphanumeric` and `to_lowercase` depend on the Unicode version. [Unicode stability policy](https://www.unicode.org/policies/stability_policy.html) keeps existing case pairs, but new characters can gain mappings. | Search tokens use the Rust toolchain's tables. Native and wasm builds from one toolchain agree. Reimplementations must use the same Unicode version (17.0.0). |
| Text length | `String.length` counts UTF-16 units | Lengths count Unicode scalar values. |
| Integer width | `usize` is 32 bits on wasm32 | Counts and sums use `u64`/`u128`. |
| Clock | Results change with time | `policy.now` only. |
| Insertion order | Different stores see records in different orders | Every rule is defined on the set of records. Fork evidence is canonical (5.2). |
| Sparse vs full store | A sparse store lacks unrelated records | Views over the related set (§14) equal views over the full store. Tested. |

## 13. Constants

Every constant used by the views. Changing one changes view output and needs a new version of this document.

| Constant | Value | Section |
|---|---|---|
| Dirichlet prior | 1 pseudo-rating per star (weight 5, mean 3) | §11.3 |
| Rating points scale | 10 per star above or below 3 | §11.3 |
| Offer points | 5 | §11.3 |
| Text points | gtin 100; name 30/15; mpn 25/10; brand 20/10; category 10/5 | §11.2 |
| Minimum prefix length | 2 characters | §11.2 |
| Query tokens | at most 16 | §11.1 |
| Search limit | default 100, at most 1,000 | §11.6 |
| Price-feeding availability | `in_stock`, `preorder` | §8.3 |

## 14. Related set

`related_records(store, key, &policy)` returns the sorted, deduplicated IDs of every record that `product_view(key)` reads (8.1). Views over a sparse store holding only these records equal views over the full store, for this policy and for any policy that trusts a subset of the same attestors.

The set is the closure of these rules:

1. **Product records**: every member of every product, listing, and review chain with product key `key`; every `observation` with `product = key` or with `listing` resolving to one of those listings; every `redact` record with `product = key`.
2. **Chains**: for every included record of a replaceable kind, every member of its chain. For every reference field (`target`, `listing`, `receipt.listing`) in an included record, the referenced record and its chain.
3. **Targets**: for every member of an included listing, review, product, or profile chain: `flag`, `appeal`, `retract`, and `redact` records whose `target` is that member. For every included record: `retract` and `redact` records targeting it.
4. **Involved authors**: authors of included records; authors of receipt listings.
5. **Author data**, for every involved author: all `profile` chains; all `retire` records; fork evidence; all `attest.identity`, `attest.domain`, and `attest.revoke` records with that subject.
6. **Attestors**: for every trusted attestor and every author of an included attestation: `retire` records and fork evidence.
7. **Persons**: for every pair `(A, p)` with `A` trusted for `attest.identity` in an included `attest.identity`, every `attest.identity` by `A` with `person = p`. Their subjects are involved authors. This rule ignores expiry and retraction, so the set does not depend on `now`.
8. **Restriction**: for every person of a listing author for `key`, every flagged listing chain of every key of that person, with its flags. Unflagged listings cannot be hidden (§6), so they are left out.
9. Rules 3 to 7 apply to the flaggers found in rules 3 and 8, so their verification and persons are known.

perch-views computes the set by running `product_view` with a recording store accessor: every record ID that a lookup returns is added. Because a sparse store returns a subset of what the full store returns for each lookup, and every returned ID is in the set, each lookup gives the same result over the sparse store. Tests check byte equality for 54 products of the Small and Medium demo datasets.

Measured sizes: 1 to 79 records on Small; 14 to 133 records on Medium; 193 to 262 records for the 3 most-reviewed products on Large.

Rule 7 follows only trusted attestors. Untrusted keys could otherwise attest each other and link any key to millions of others, which makes the set unbounded. A client whose policy adds attestors that the node's policy lacks can get an incomplete set (threat-model R11).

## 15. Other functions

### 15.1 AuthorView

`author_view(store, policy, author)` returns `null` unless the store holds a record by `author` or an attestation with `subject = author`.

| Field | Value |
|---|---|
| `id` | The author |
| `name`, `role`, `url`, `about` | §10.1 |
| `profile`, `profile_status` | Moderation of the profile chain (§7.6) and its status, or `null` |
| `verified` | From the author's counting `attest.identity` with the lowest `(attestor, person, root)`: `{ level, legal_name|null, country|null, attestor, expires }`, or `null` |
| `domains` | One entry `{ domain, attestor, expires }` per counting `attest.domain`, sorted by `(domain, attestor)` |
| `status` | §5.4 |
| `restricted`, `restriction` | §6. `restriction` is `{ reason, hidden_listings, distinct_flaggers }` of the author's person, or `null` when not restricted and `hidden_listings` = 0. `reason` is `forked_key` when the person is restricted only because a key forked (§6 condition 2), otherwise `hidden_listings`. |
| `revoked` | Reported revoke `{ attestor, valid_through_seq }` (§5.3), or `null` |
| `fork_seq`, `fork_evidence` | From the evidence pair: fork seq and `[a.id, b.id]` sorted by `(seq, id)`; or `null` |
| `retired` | `{ seq: retire_seq, successor|null }` when `retire_seq < ∞`, else `null` |
| `record_count` | Number of the author's active records |
| `hidden_listing_count` | Number of the author's listing chains with status `hidden` |
| `seller_reviews` | `{ total, with_valid_receipt }`. The products this author lists are the reference products of the author's listing chains whose reference version is active. `total` counts the counted reviews (§9.3) of those products. `with_valid_receipt` counts those whose receipt passes rows 2 to 4 of §9.2 with this author as the listing's seller. A low ratio can show that a seller gives receipts selectively (threat-model R8). |

### 15.2 author_records

`author_records(store, policy, author, kinds, limit, from_seq)` returns `[ChainSummary]` for chains whose root is by `author`, has `root.seq ≥ from_seq`, and has a kind in `kinds` (all kinds if empty). Results are sorted by root seq and limited to `limit`.

```text
ChainSummary { root, latest, kind, ts, status, product|null, title|null }
```

- Chains with a latest version: `latest` and `ts` from it. `status` per §3.6 for the four display kinds; for other kinds `retracted`, `unavailable`, or `visible`.
- Chains whose members are all invalid: `status: "invalid"`, `latest` = the highest-seq active member, `ts` from it.
- Chains with no active member are omitted.
- `product`: the product key for `product`, `listing`, `review`, and `observation` chains. `title`: `listing.title`, `review.title`, `product.name`, or `profile.name` from the latest version; otherwise `null`.

### 15.3 record_status

`record_status(store, policy, id)` returns `null` for an unknown ID, otherwise:

```text
RecordStatus { semantic_valid, reason|null, root, versions: [id], ignored|null }
```

- `semantic_valid` and `reason` per §4. `semantic_valid` is false for unavailable records and unknown kinds, with reason `unavailable` or `unknown_kind`.
- `versions`: the chain's candidates (§3.5) in seq order.
- `ignored`: why views ignore an otherwise valid record: `future`, `forked`, `revoked`, `retired`, `retracted` (a replacement after a retraction), or `not_after_seq`; `null` otherwise.

### 15.4 Stats

`stats(store, policy)` counts the store. Only `products` depends on validity rules, and none of the counts depend on the policy.

| Field | Value |
|---|---|
| `records` | Records held, including evidence records |
| `authors` | Distinct authors of held records |
| `products` | Distinct product keys of product chains with a reference version |
| `listings`, `reviews` | Chains of that kind with a reference version |
| `forks` | Authors with fork evidence |
| `invalid` | Semantically invalid records (§4), excluding evidence records |
| `redacted` | Records without a body |
| `unknown` | Records of unknown kinds |

### 15.5 Admission tier

Tiers depend on identity and references only, not on product facts. A product whose only sources are unverified authors can therefore be evicted from nodes short of space (contract section 9; threat-model R15).

`admission_tier(store, admission, author) → 1 | 2 | 3` implements contract section 9. `admission` is a `Policy` built from the node's admission attestor set and the current time.

1. **Tier 1**: the author is in `admission.trusted_attestors`, or is verified under `admission` (§5.2).
2. **Tier 2**: the author has a stored record that a tier-1 author's active record references. References are: the target of a flag or appeal; `review.listing`; `review.receipt.listing`.
3. **Tier 3**: all others.

`referenced_by_tier1(store, admission, id)` answers rule 2 for a single record ID. A node can then classify an incoming log before storing it.

### 15.6 Reusing results across queries

`Views::new(store, policy)` memoizes intermediate results for one batch of queries. `Views::into_cache()` returns them as a `ViewCache`. `Views::with_cache(store, policy, cache)` reuses them. A cache is valid only for unchanged store contents and an equal policy. Callers discard it after any insert, body fill, or body removal. As a safety net, `with_cache` discards a cache whose policy, record count, body-less count, or fork evidence differ. Results with and without a cache are byte-identical.

### 15.7 Performance

Measured on the Large demo dataset (99,457 records, 4,409 product keys), native release build, one thread, CPU time of the thread, minimum of 5 runs, 2026-10-18. Command: `cargo run --release -p perch-views --example bench -- large`.

| Operation | Time | Target |
|---|---|---|
| `product_view`, 18 products (up to 102 reviews) | mean 1.05 ms, max 3.3 ms | 20 ms |
| `search`, GTIN query (1 hit) | 70 ms | 200 ms |
| `search`, two words (22 hits) | 81 ms | 200 ms |
| `search`, one word (100 of 170–440 hits) | 132–191 ms | 200 ms |
| `search`, empty query (100 of 4,409 hits) | 865 ms | 200 ms |
| `search`, repeated narrow query with a kept `ViewCache` | 65 ms | — |
| `stats` | 800 ms | — |
| `related_records`, most-reviewed products | 4–10 ms | — |

Cost per search is at least one pass over all product records (text matching), plus rating and price work for each hit that can reach the first `limit`. A profile of the empty query attributes 29% of instructions to body schema checks and 19% to receipt signature checks for the returned hits. Callers that list all products should keep a `ViewCache` (§15.6) or use a narrower query.

## 16. Research notes

### 16.1 Rating aggregation

| Method | Uses | Weakness |
|---|---|---|
| Arithmetic mean | `RatingSummary.mean` | Ignores sample size ([Miller 2009](https://www.evanmiller.org/how-not-to-sort-by-average-rating.html)). |
| Wilson lower bound | Not used | For two-valued votes only; needs square roots. |
| Bayesian (Dirichlet) mean | Search ranking | Prior is a choice; it is published in §13. |
| Bayesian lower bound ([Miller 2014](https://www.evanmiller.org/ranking-items-with-star-ratings.html), [2012](https://www.evanmiller.org/bayesian-average-ratings.html)) | Not used | Needs square roots or the incomplete beta function, which are floating-point. |
| Median rating | Not used | On a 5-point scale it moves in whole stars and hides changes. |

### 16.2 Brigading

- Steam detects review bombs and excludes off-topic reviews from the score after moderator review, without deleting them ([Engadget, 2019-03-16](https://www.engadget.com/2019/03/16/steam-review-bombing/)). Perch has no moderators who decide in private, so it excludes by public rules instead.
- [Spognardi and Petrocchi (2020)](https://research.uniroma1.it/node/53937) show that changing the aggregation function can limit the damage from injected ratings without detecting fake reviewers.
- Perch's rules: ranking uses only identity-verified reviews, one per person (7.8); sellers' reviews of their own products do not count (§9.3); a brigade of unverified keys changes only the secondary all-reviews numbers. The demo dataset includes such a brigade, and a test checks that it does not change the search rank.
- Remaining exposure: a brigade of real, verified people (threat-model R9, moderation.md 3.2).

### 16.3 Conflicting facts

Values are ranked by verified-person support, then all-person support, then alphabetically (§10.4). Majority over persons is the simplest rule a user can check by counting the `sources` lists. TruthFinder-style methods were rejected (§10.4).

### 16.4 Prices

- Median, not mean, for robustness (§10.5).
- No currency conversion (§10.5).
- Hidden listings, restricted sellers, unavailable offers, and (by default) domains without attestation are excluded from price summaries (§8.3), because the lowest price is where scams concentrate (threat-model R5).

### 16.5 Sybil resistance

Without a central identity authority, one entity can present many identities ([Douceur 2002](https://www.microsoft.com/en-us/research/publication/the-sybil-attack/)). Every count that affects ranking, hiding, restriction, or facts is a count of verified persons (§5.2), never of keys. Counts of all keys appear only as secondary information.

## Appendix A: terms and acronyms

| Term | Meaning |
|---|---|
| Active record | A record not cut off by time, fork, revoke, or retirement (§3.2). |
| Brigade | Many accounts or people acting together to push a rating or moderation outcome. |
| Candidate | A chain member that can be the latest version (§3.5). |
| Dangling reference | A reference to an ID the store does not hold. |
| Dirichlet prior | A probability model that adds pseudo-counts to each rating level before averaging. |
| DSA | EU Digital Services Act, Regulation (EU) 2022/2065. |
| Excluded-forked | An author with fork evidence and no counting revoke (§5.4). |
| FX | Foreign exchange (currency conversion). |
| IDNA | Internationalized Domain Names in Applications; ToASCII converts a host to ASCII (punycode). |
| IEEE 754 binary64 | The 64-bit floating-point format (`f64`, JavaScript `Number`). |
| ISO 4217 | Standard list of currency codes. |
| JCS | JSON Canonicalization Scheme, RFC 8785. |
| Lower median | For an even count, the lower of the two middle values. |
| Person | A connected component of keys linked by shared attestation pairs (§5.2). |
| Reference version | The earliest valid member of a chain with a body (§3.4). |
| Related set | The records needed to compute one product's view (§14). |
| Round half to even | Rounding where an exact half goes to the even neighbor. |
| Source chain | A product chain whose values feed facts (§10.3). |
| Sparse store | A store holding records without complete log prefixes (5.5). |
| Union-find | A data structure that merges sets and finds a set's representative. |
| ViewCache | Memoized view results kept between queries over an unchanged store (§15.6). |
