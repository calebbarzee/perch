# Perch data model

Status: specification for contract version 3 (2026-10-18). Owner: data-modeler. Implementation: [`crates/perch-core`](../crates/perch-core). Test vectors: [`test-vectors.json`](test-vectors.json).

This document specifies the record format precisely enough for a third party to read, verify, and write Perch records in any language. Section numbers such as "4.2" refer to [`ARCHITECTURE.md`](../ARCHITECTURE.md); "§3" refers to this document. When this document conflicts with `ARCHITECTURE.md`, the contract wins.

## 1. Overview

- A **record** is one JSON object: a signed **header**, a **signature**, and an optional **body**.
- The **ID** of a record is the SHA-256 hash of its canonical header.
- The header contains `body_hash`, the SHA-256 hash of the canonical body. The signature therefore covers the body without containing it.
- A node can remove a body (§9) and keep the header. The ID, signature, and log stay valid.
- Each author signs an append-only **log**: record `seq = n` names the ID of record `n − 1` in `prev`.
- The dataset is a file of records, one per line ([JSON Lines](https://jsonlines.org)).

Standards used:

| Standard | Use |
|---|---|
| [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259) | JSON syntax |
| [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) | Canonical JSON (JCS), restricted to integers |
| [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032) | Ed25519 signatures (pure Ed25519, no context) |
| [FIPS 180-4](https://csrc.nist.gov/pubs/fips/180-4/upd1/final) | SHA-256 |
| [WHATWG URL](https://url.spec.whatwg.org/) | URL parsing |
| [UTS #46](https://www.unicode.org/reports/tr46/) | IDNA ToASCII for hostnames |
| [ISO 4217](https://www.iso.org/iso-4217-currency-codes.html) | Currency codes and minor units |
| [ISO 3166-1](https://www.iso.org/iso-3166-country-codes.html) | Country codes (alpha-2) |
| [GS1 GTIN](https://www.gs1.org/standards/id-keys/gtin) | Product numbers and check digits |

## 2. Record fields

| Field | Part | JSON type | Rule |
|---|---|---|---|
| `v` | header | integer | Always `1`. |
| `author` | header | string | Ed25519 public key: 64 lowercase hex characters. |
| `seq` | header | integer | 1 to 2^53 − 1. Position in the author's log. |
| `prev` | header | string or `null` | 64 lowercase hex: ID of the author's record at `seq − 1`. `null` if and only if `seq` is 1. |
| `ts` | header | integer | 0 to 2^53 − 1. Time claimed by the author, Unix milliseconds. |
| `kind` | header | string | Matches `^[a-z][a-z0-9_.]{0,31}$`. |
| `replaces` | header | string | Optional; omitted when absent. 64 lowercase hex. `null` is invalid. |
| `body_hash` | header | string | 64 lowercase hex: SHA-256 of the canonical body. |
| `sig` | signature | string | 128 lowercase hex: Ed25519 signature. |
| `body` | body | object | Optional. Kind-specific content (§5). |

Any other top-level field makes the record invalid. Uppercase hex is invalid everywhere.

Example (vector `profile_seq1`; one line in a file, wrapped here):

```json
{"author":"0708b156a92bfe321e81f2914570e17fa54b25c3ac6adafbd1db4be349e2d991",
 "body":{"name":"Ada's Kettles","role":"seller","url":"https://kettles.example/"},
 "body_hash":"34eabb1e461486a7c3eb41acf52e4cfdb0158d312e08d5a2f94f9656fb096386",
 "kind":"profile","prev":null,"seq":1,
 "sig":"9826a134860d827d74caba269119ef33cc29545c8c73b3b566c1f2fbdd5a17d1a2b954f86680ac012659482a500d191030ef1bdd6d8358c6a66b0ab2ad66910e",
 "ts":1760000000000,"v":1}
```

## 3. Canonical JSON

### 3.1 Encoding

The canonical encoding of a JSON value is RFC 8785 restricted to integers:

1. No whitespace outside strings.
2. Object members sorted by key, comparing keys as sequences of UTF-16 code units.
3. Numbers: integers from −(2^53 − 1) to 2^53 − 1, written in decimal without leading zeros, `+`, fraction, or exponent.
4. Strings: UTF-8. Escape `"` as `\"` and `\` as `\\`. Escape U+0008, U+0009, U+000A, U+000C, U+000D as `\b \t \n \f \r`. Escape other characters below U+0020 as `\u00xx` with lowercase hex. Write every other character as itself, including `/`, U+007F, and non-ASCII.
5. `true`, `false`, `null` as written.

UTF-16 order differs from UTF-8 byte order only between characters at or above U+10000 and characters in U+E000–U+FFFF. Example: key `"😀"` (U+1F600, UTF-16 `D83D DE00`) sorts before key `"\u{E000}"` (UTF-16 `E000`). Vector `unknown_kind_header_only_source` contains both.

### 3.2 Parsing rules for input

A Perch reader rejects input that a lenient JSON parser would accept:

| Rule | Examples rejected |
|---|---|
| A record line above 131,072 bytes is rejected before parsing. | — |
| Duplicate keys at any depth. `"a"` and `"a"` are the same key. | `{"a":1,"a":2}` |
| Number tokens with a fraction or exponent, even if integral. | `1.0`, `1e0`, `10E-1` |
| The token `-0`. | `-0` |
| Integers outside ±(2^53 − 1). | `9007199254740992` |
| `\u` escapes that form a lone surrogate. | `"\ud800"`, `"\udc00x"` |
| Nesting depth above 16. The record object is depth 1; each nested object or array adds 1. A record's `body` object is depth 2. | 17 nested arrays |
| Byte order mark, or data after the value. | `{}{}` |

Whitespace between tokens and optional escapes (`\/`, `A`, uppercase hex in `é`) are valid input. Readers normalize them away: the stored form is always canonical (§4).

### 3.3 Size limits

| Item | Limit |
|---|---|
| Record line before parsing | 131,072 bytes |
| Canonical header | 2,048 bytes |
| Canonical body | 65,536 bytes |
| Nesting depth | 16 |

## 4. ID, signature, and wire form

### 4.1 Definitions

```text
body_canonical   = canonical_json(body)                         ≤ 65,536 bytes
body_hash        = lowercase_hex(sha256(body_canonical))
header           = { v, author, seq, prev, ts, kind, replaces?, body_hash }
header_canonical = canonical_json(header)                       ≤ 2,048 bytes
id               = lowercase_hex(sha256(header_canonical))
sig              = ed25519_sign(secret_key, "perch/record/v1\n" || header_canonical)
wire             = canonical_json(header + { sig } + { body }?)
```

`"perch/record/v1\n"` is 16 ASCII bytes ending in a line feed (0x0A).

Because all header keys are ASCII, the canonical header has a fixed key order: `author`, `body_hash`, `kind`, `prev`, `replaces` (if present), `seq`, `ts`, `v`. The wire form adds `body` after `author` and `sig` after `seq`.

### 4.2 Signature domains

| Prefix | Signs |
|---|---|
| `perch/record/v1\n` | Record headers |
| `perch/receipt/v1\n` | Receipts (§6) |

A signature made for one domain never verifies in another. Tests: `keys::tests::domain_separation`, `receipt::tests::sign_and_verify`.

### 4.3 Strict Ed25519 verification

A verifier must reject a signature unless all of these hold ([RFC 8032 section 5.1.7](https://www.rfc-editor.org/rfc/rfc8032#section-5.1.7) plus the checks in 4.2):

1. `A` (the 32-byte public key) and `R` (the first 32 bytes of `sig`) are canonical encodings: the low 255 bits, read as a little-endian integer `y`, satisfy `y < p` where `p = 2^255 − 19`.
2. Neither `A` nor `R` has small order.
3. `S` (the last 32 bytes of `sig`, little-endian) is less than `L = 2^252 + 27742317777372353535851937790883648493`.
4. `[S]B = R + [k]A` without multiplying by the cofactor, where `k = SHA-512(R || A || message) mod L`.

`perch-core` uses `verify_strict` from [ed25519-dalek](https://docs.rs/ed25519-dalek) 3.0 and adds check 1, which dalek does not apply to `A` or `R`.

Measured behavior of common libraries (2026-10-18):

| Library | Rejects `S ≥ L` | Rejects small-order `A` or `R` | Rejects non-canonical `A` or `R` |
|---|---|---|---|
| ed25519-dalek 3.0 `verify_strict` | yes | yes | no |
| Node.js 22.22 Web Crypto | yes | no | not tested |
| Python `cryptography` 50.0.1 (OpenSSL) | yes | no | not tested |

With Web Crypto and `cryptography`, public key `01 00…00` and signature `01 00…00 00…00` verify as valid for every message. Verifiers using these libraries must apply checks 1 and 2 themselves. In canonical encodings, the 8 small-order points have exactly these `y` values: `0`, `1`, `p − 1`, `Y8`, and `p − Y8`, where `Y8 = 2707385501144840649318225287225658788936804267575313519463743609750303402022`. The snippets in §11 check them.

### 4.4 Wire form and normalization

- The wire form is the stored and relayed form, and one JSONL line.
- A reader accepts any valid input encoding and stores the wire form. Two encodings of the same record have the same ID.
- The body is stored as received JSON, including fields the reader does not understand. Never re-encode a body from a typed structure: an unknown field would be lost and `body_hash` would no longer match.
- A record without a body (header-only) has the same ID and signature. Vector `header_only` is vector `unknown_kind_header_only_source` without its body.

## 5. Kinds

### 5.1 Common rules

These rules are semantic (§7). A record that breaks them stays stored and relayed; views ignore it.

| Rule | Detail |
|---|---|
| Text length | Counted in Unicode scalar values (code points excluding surrogates). `"é"` is 1; `"e"` + U+0301 is 2; `"😀"` is 1. |
| Optional fields | Absent when not given. `null` is invalid. |
| Unknown fields | Allowed and ignored in bodies and nested objects, except inside a receipt. |
| Integers | 0 to 2^53 − 1 unless stated. |
| `Id`, `Author`, `Hex64` | 64 lowercase hex characters. |
| Country | Two ASCII uppercase letters (ISO 3166-1 alpha-2 syntax; assignment is not checked). |
| `Price` | `{ "amount": integer ≥ 0 in minor units, "currency": code }`. The code must be in the ISO 4217 table of `perch-core` with defined minor units (§5.4). |
| `URL` | §5.5. |
| `ProductKey` | §8. |

### 5.2 Body schemas

| Kind | Fields (`?` = optional; lengths in scalar values) |
|---|---|
| `profile` | `name` 1–100; `role` one of `seller`, `buyer`, `tester`, `crawler`, `attestor`, `node`, `other`; `url`? URL; `about`? ≤ 2000 |
| `attest.identity` | `subject` Author; `person` Hex64; `level` `person` or `business`; `legal_name`? 1–200, only when `level` is `business`; `country`? alpha-2; `expires` Unix ms |
| `attest.domain` | `subject` Author; `domain` hostname (§5.5); `expires` Unix ms |
| `attest.revoke` | `subject` Author; `valid_through_seq` integer ≥ 0 |
| `product` | `name` 1–300; `brand` 1–100; `gtin`? string of 8, 12, 13, or 14 ASCII digits with a valid check digit; `mpn`? 1–100; `category`? (below); `attributes`? object of ≤ 64 entries, keys 1–64, values strings ≤ 500; `images`? ≤ 16 × `{ url: URL, sha256?: Hex64 }`; `description`? ≤ 5000 |
| `listing` | `product` ProductKey; `url` URL; `price` Price; `availability` one of `in_stock`, `out_of_stock`, `preorder`, `discontinued`; `condition` one of `new`, `used`, `refurbished`; `title`? ≤ 300; `ships_to`? ≤ 250 alpha-2 codes; `location`? `{ country: alpha-2, region?: ≤ 100 }` |
| `observation` | `product` ProductKey; `url` URL; `price` Price; `observed_at` Unix ms, ≤ the record's `ts`; `listing`? Id; `availability`? as for listing |
| `review` | `product` ProductKey; `rating` integer 1–5; `text` 1–10000; `title`? ≤ 200; `listing`? Id; `receipt`? Receipt (§6) |
| `flag` | `target` Id; `reason` one of `fraud`, `counterfeit`, `misleading`, `spam`, `illegal`, `personal_data`, `other`; `note`? ≤ 2000 |
| `appeal` | `target` Id; `note` 1–2000 |
| `retract` | `target` Id |
| `retire` | `successor`? Author |
| `redact` | `target` Id; `reason` one of `illegal`, `personal_data`, `court_order`, `other`; `note`? ≤ 2000 |

`product.category`: 1–200 scalar values; segments separated by `/`; each segment non-empty with no leading or trailing ASCII whitespace; no ASCII uppercase letters. Non-ASCII letters are not case-checked, so the rule does not depend on a Unicode version. `perch-core` adds one rule to `product`: with `mpn` and no `gtin`, the `mpn:` key must be derivable (brand and MPN not blank after trimming).

Kinds not in the table are valid records. Views ignore them.

### 5.3 Kind properties

| Kind | Replaceable | Fixed across versions | Control | Can be retracted |
|---|---|---|---|---|
| `appeal` | yes | `target` | no | yes |
| `attest.domain` | yes | `subject`, `domain` | yes | yes |
| `attest.identity` | yes | `subject`, `person` | yes | yes |
| `attest.revoke` | no | — | yes | no |
| `flag` | yes | `target` | no | yes |
| `listing` | yes | `product` | no | yes |
| `observation` | no | — | no | yes |
| `product` | yes | `gtin`, `mpn`; `brand` when `mpn` is set | no | yes |
| `profile` | yes | — | no | yes |
| `redact` | no | — | no | no |
| `retire` | no | — | yes | no |
| `retract` | no | — | yes | no |
| `review` | yes | `product` | no | yes |

- A record of a non-replaceable kind with `replaces` is semantically invalid.
- A fixed field must have the same JSON value, and the same presence, in every version of a chain.
- A node must not remove the body of a control-kind record (§9).
- Rust: `kinds::KINDS`, `kind_info`, `is_replaceable`, `fixed_fields`, `check_fixed_fields`, `is_control_kind`, `is_retractable`.

### 5.4 Currencies

`perch_core::currency::CURRENCIES` lists the 178 active ISO 4217 codes. Source: the SIX list as mirrored in [`datasets/currency-codes`](https://github.com/datasets/currency-codes) (`data/codes-all.csv`, retrieved 2026-10-18). Codes with a withdrawal date are excluded (for example `BGN`, withdrawn 2026-01).

| Minor units | Codes (examples) | Count |
|---|---|---|
| 0 | `JPY`, `KRW`, `ISK` | 17 |
| 2 | `EUR`, `USD`, `GBP` | 139 |
| 3 | `BHD`, `KWD`, `TND` | 7 |
| 4 | `CLF`, `UYW` | 2 |
| none (N.A.) | `XAU`, `XDR`, `XXX` | 13 |

A `Price` must use a code with defined minor units: `amount` counts minor units, so `{ "amount": 1999, "currency": "EUR" }` is 19.99 EUR and `{ "amount": 1999, "currency": "JPY" }` is 1999 JPY.

### 5.5 URLs and hostnames

A `URL` field is valid when all hold:

1. At most 2048 scalar values.
2. No ASCII control character, space, or `\`. (The WHATWG parser strips or rewrites these, so the shown and parsed URL would differ.)
3. The WHATWG URL parser accepts it.
4. The scheme is `https`.
5. No username or password.
6. The host is a domain, not an IPv4 or IPv6 address. The parser applies IDNA ToASCII.

The record keeps the original string. `perch_core::https_url_host` returns the ASCII host for domain matching: `https://Bücher.example/x` → `xn--bcher-kva.example`.

An `attest.domain` `domain` is a lowercase ASCII hostname: at most 253 bytes; at least 2 labels; each label 1–63 characters of `a-z`, `0-9`, `-`, not starting or ending with `-`; the last label not all digits. A URL host matches a domain when it equals the domain or ends with `.` + domain (`weburl::host_matches_domain`).

## 6. Receipts

A receipt is a detached object signed by a seller. It is not a log record.

```json
{"buyer":"3a4e26c6cb87a55ecad0e1a1d2fe8170a81f1117f53373ce4610ecbdbc8965c5",
 "listing":"8b7c1f47af27c72d5705dde1e2f98458839eacfad31b80d664c386dacdb20e4a",
 "nonce":"abadd683af89a28428f1ce9df2c4df0537b8e6a6b52085fad6ebf22dae720856",
 "purchased_at":1760000100000,
 "sig":"4bcbdd61367ef186cd2e7db589a060cc08bc2523a8b42a87f027c82399359adf1384a7f6efb7a6078371e503b9ada3345472a0f29bc95d4e6b9299ced815f509"}
```

```text
signed = canonical_json({ listing, buyer, purchased_at, nonce })
sig    = ed25519_sign(seller_secret_key, "perch/receipt/v1\n" || signed)
```

- `listing`: root ID of the seller's listing. `buyer`: Author. `purchased_at`: Unix ms. `nonce`: 32 random bytes as Hex64.
- Fields other than these five are invalid inside a receipt.
- `perch-core` checks only the signature (`verify_receipt(receipt, seller)`). Views decide who the seller is and whether the review counts as a verified purchase (7.5).
- The caller supplies the nonce, because `perch-core` has no random source on wasm.

## 7. Validity levels

| Level | Checked by | Rules | Failure |
|---|---|---|---|
| Isolated | `perch-core` (`Record::from_json_str`) | §2, §3, §4: parsing, fields, types, sizes, `kind` format, `body_hash` when the body is present, signature | Rejected; never stored or relayed. |
| Log | `perch-store` | `seq`/`prev` continue the log; `ts` ≥ the previous record's `ts` | Gap: held. `ts` decrease: rejected. Conflict: fork (§10). |
| Semantic | `perch-views`, using `perch_core::kinds` | §5 schemas; references; permissions; time rules | Stored and relayed; ignored by views. |

`kinds::check_record(&Record)` applies the body schema plus three rules that need the header: the body is present; a non-replaceable kind has no `replaces`; `observation.observed_at ≤ ts`. Errors serialize as `{ "field": "price.currency", "problem": "bad_currency" }`.

## 8. Product keys

A `ProductKey` groups records about one product. Records match keys by exact string equality, so a key field must be in canonical form.

| Priority | Product has | Key | Example |
|---|---|---|---|
| 1 | `gtin` | `gtin:` + GTIN left-padded with `0` to 14 digits | `gtin` `"96385074"` → `gtin:00000096385074` |
| 2 | `mpn` | `mpn:` + brand + `:` + MPN, each with ASCII whitespace (U+0009, U+000A, U+000C, U+000D, U+0020) trimmed and ASCII letters lowercased, then `%` written as `%25` and `:` as `%3a` | brand `" ACME Corp\t"`, mpn `" WH-1000XM5 "` → `mpn:acme corp:wh-1000xm5` |
| 3 | neither | `rec:` + root ID of the product chain | `rec:` + 64 hex |

- Non-ASCII letters keep their case: brand `MÜLLER`, mpn `Ä-1` → `mpn:mÜller:Ä-1`.
- GTIN check digit: weight the digits left of the check digit 3, 1, 3, … from the right; the check digit is `(10 − sum mod 10) mod 10`.
- A canonical key field: `gtin:` + exactly 14 digits with a valid check digit; or `mpn:` + two escaped parts separated by the only `:`, each non-empty after unescaping, without leading or trailing ASCII whitespace, and without ASCII uppercase; or `rec:` + 64 lowercase hex. In a part, `%` may appear only as `%25` or `%3a`.
- Brand and MPN may contain `:` and `%`. Escaping keeps keys unique: brand `a:b` with MPN `c` gives `mpn:a%3ab:c`; brand `a` with MPN `b:c` gives `mpn:a:b%3ac`; brand `100% Cotton` with MPN `X` gives `mpn:100%25 cotton:x`.
- `test-vectors.json` → `product_keys` lists 5 derivations.

## 9. Bodies and redaction

- A record with no `body` field is header-only. It is isolated-valid if its header and signature verify.
- A store that holds a header-only record accepts the body later when `sha256(canonical_json(body)) = body_hash` (`Record::with_body`).
- A node keeps a local list of removed IDs and refuses bodies for them. It must not remove bodies of control kinds (§5.3).
- Views treat a header-only record as unavailable.

## 10. Logs and forks

- Each author has one log: `seq` 1, 2, 3, … with each `prev` naming the previous ID.
- To publish, an author uses `seq = head_seq + 1` and `prev = head_id` (`fork::next_position`), and `ts` ≥ the head's `ts`.
- A **fork** is evidence that an author signed two histories. Two records `x`, `y` by the same author are an **evidence pair** when either:
  1. `x.seq = y.seq` and `x.id ≠ y.id`; or
  2. `y.seq = x.seq + 1` and `y.prev ≠ x.id`.
- A pair is written `(a, b)`, sorted by `(seq, id)`. Its fork seq is `a.seq`.
- Pairs are totally ordered by `(fork seq, a.seq, a.id, b.seq, b.id)`, IDs compared as hex strings. Stores keep the lowest (5.2).
- A pair verifies without bodies.
- Rust: `EvidencePair::new(x, y)` checks the rules and sorts; `Ord` implements the total order. Serialized form: `[record a, record b]`.

Store behavior (complete-prefix logs, gap buffer, truncation at the fork seq, sparse stores) is specified in [`sync-protocol.md`](sync-protocol.md) section 6 and implemented in `perch-store`.

## 11. Verifying a record with standard tools

### 11.1 Steps

1. Parse the line with the rules in §3.2.
2. Check the field rules in §2.
3. Remove `sig` and `body`. Encode the rest canonically: this is `header_canonical`.
4. `id = sha256(header_canonical)`.
5. If `body` is present, check `sha256(canonical_json(body)) = body_hash`.
6. Verify `sig` over `"perch/record/v1\n" || header_canonical` under `author`, with the checks of §4.3.

### 11.2 Node.js (Web Crypto, no packages)

Tested with Node.js 22.22.0 against every record vector (7 of 7 IDs and signatures reproduced) and against `invalid` vectors and a small-order forgery (all signature cases rejected).

```js
// node verify.mjs record.json
import { readFileSync } from "node:fs";
const { subtle } = globalThis.crypto;

// Canonical JSON (RFC 8785, integers only). JSON.stringify escapes strings
// as Perch requires. Array.prototype.sort orders keys by UTF-16 code units.
function canon(v) {
  if (Array.isArray(v)) return "[" + v.map(canon).join(",") + "]";
  if (v !== null && typeof v === "object")
    return "{" + Object.keys(v).sort().map(k => JSON.stringify(k) + ":" + canon(v[k])).join(",") + "}";
  if (typeof v === "number" && !Number.isSafeInteger(v)) throw new Error("not a safe integer");
  return JSON.stringify(v);
}

const hex = b => Buffer.from(b).toString("hex");
const sha256 = async s => hex(await subtle.digest("SHA-256", new TextEncoder().encode(s)));

// Strict Ed25519 (section 4.3): Web Crypto rejects S >= L but accepts
// small-order and non-canonical points, so check A and R here.
const P = 2n ** 255n - 19n;
const Y8 = 2707385501144840649318225287225658788936804267575313519463743609750303402022n;
const SMALL_ORDER_Y = [0n, 1n, P - 1n, Y8, P - Y8];
function pointOk(bytes) {
  let y = 0n;
  for (let i = 31; i >= 0; i--) y = (y << 8n) | BigInt(bytes[i]);
  y &= (1n << 255n) - 1n; // drop the sign bit of x
  return y < P && !SMALL_ORDER_Y.includes(y);
}

const rec = JSON.parse(readFileSync(process.argv[2], "utf8"));
const { sig, body, ...header } = rec;
if (body !== undefined && (await sha256(canon(body))) !== header.body_hash)
  throw new Error("body_hash mismatch");
const headerCanonical = canon(header);
const A = Buffer.from(header.author, "hex"), S = Buffer.from(sig, "hex");
const key = await subtle.importKey("raw", A, "Ed25519", false, ["verify"]);
const ok = pointOk(A) && pointOk(S.subarray(0, 32)) &&
  await subtle.verify("Ed25519", key, S, new TextEncoder().encode("perch/record/v1\n" + headerCanonical));
console.log(JSON.stringify({ id: await sha256(headerCanonical), valid: ok }));
```

Limits of this snippet: `JSON.parse` keeps the last of duplicate keys, reads `1.0` as `1`, and accepts lone surrogates. It verifies the signature correctly, but it does not enforce the parsing rules of §3.2. Use it to check data, not as a node's input filter.

### 11.3 Python (`cryptography` package)

Tested with Python 3.13 and `cryptography` 50.0.1 against the same vectors with the same results. It also rejects duplicate keys and non-integer number tokens.

```python
# python3 verify.py record.json
import hashlib, json, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

def canon(v):
    if isinstance(v, dict):  # keys sorted by UTF-16 code units (RFC 8785)
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(canon(k) + ":" + canon(v[k]) for k in keys) + "}"
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, int) and not isinstance(v, bool) and abs(v) > 2**53 - 1:
        raise ValueError("integer out of range")
    return json.dumps(v, ensure_ascii=False)  # strings, integers, true, false, null

# Strict Ed25519 (section 4.3): reject non-canonical and small-order A and R.
P = 2**255 - 19
Y8 = 2707385501144840649318225287225658788936804267575313519463743609750303402022
def point_ok(b):
    y = int.from_bytes(b, "little") & (2**255 - 1)
    return y < P and y not in (0, 1, P - 1, Y8, P - Y8)

def no_dupes(pairs):
    if len({k for k, _ in pairs}) != len(pairs):
        raise ValueError("duplicate key")
    return dict(pairs)

def no_float(s):
    raise ValueError("number is not an integer token: " + s)

rec = json.load(open(sys.argv[1], encoding="utf-8"), object_pairs_hook=no_dupes, parse_float=no_float)
sig = bytes.fromhex(rec.pop("sig"))
body = rec.pop("body", None)
if body is not None and hashlib.sha256(canon(body).encode()).hexdigest() != rec["body_hash"]:
    raise ValueError("body_hash mismatch")
header = canon(rec).encode()
author = bytes.fromhex(rec["author"])
if not (point_ok(author) and point_ok(sig[:32])):
    raise ValueError("small-order or non-canonical point")
Ed25519PublicKey.from_public_bytes(author).verify(sig, b"perch/record/v1\n" + header)  # raises if invalid
print(json.dumps({"id": hashlib.sha256(header).hexdigest(), "valid": True}))
```

Python's `json.dumps(ensure_ascii=False)` escapes exactly the characters §3.1 requires, so it produces canonical strings.

### 11.4 Shell (`jq`, `sha256sum`, `openssl`)

For headers, `jq -jcS` produces canonical JSON because header keys are ASCII and header values contain no escapes. Tested with jq 1.7 and OpenSSL 3.0.13 on vector `review_escapes_receipt`.

```sh
f=record.json
jq -jcS 'del(.sig, .body)' "$f" > header.txt               # header_canonical
sha256sum header.txt                                       # = id
printf 'perch/record/v1\n' | cat - header.txt > msg.bin
hex2bin() { tr 'a-f' 'A-F' | basenc --base16 -d; }
{ printf '302a300506032b6570032100'; jq -j .author "$f"; } | hex2bin > pub.der   # SPKI wrapper
jq -j .sig "$f" | hex2bin > sig.bin
openssl pkeyutl -verify -pubin -inkey pub.der -keyform DER -rawin -in msg.bin -sigfile sig.bin
```

Limits: OpenSSL does not reject small-order keys (§4.3). `jq -S` sorts keys by code point, not UTF-16, and escapes U+007F, so it does not produce canonical bodies in general (it fails on vector `unknown_kind_header_only_source`). Use §11.2 or §11.3 for `body_hash`.

## 12. JSON Lines files

- One record per line, in wire form, each line ending in `\n` (`to_jsonl`).
- Readers ignore blank lines and a trailing `\r` (`parse_jsonl`). Each line is parsed and verified in full; a bad line is reported with its 1-based line number and does not stop the read.
- The file is the persistence format of nodes and the public export (`GET /v1/export.jsonl`).

## 13. Test vectors

[`test-vectors.json`](test-vectors.json) is generated from fixed seeds by `cargo test -p perch-core --test vectors -- --ignored write_vectors` and checked by the `vectors_match_file` test.

| Section | Count | Contents |
|---|---|---|
| `records` | 7 | Each with `seed`, `author`, `header_canonical`, `signed_message_hex`, `id`, `sig`, `body_hash`, `body_canonical` (if a body), `wire`. Includes: seq 1 with `prev: null`; non-ASCII text (`product_non_ascii`); escapes `"`, `\`, `\n`, `\t`, U+0001, U+001F (`review_escapes_receipt`); `replaces`; an unknown kind with UTF-16-order keys; a header-only record. |
| `receipts` | 1 | Receipt with seller seed, `signed_canonical`, and `signed_message_hex`. |
| `invalid` | 12 | Wire strings that must be rejected, with the expected `error` tag: tampered signature and `ts`, uppercase hex, duplicate key, `1.0`, `1e0`, `-0`, lone surrogate, `replaces: null`, unknown field, body hash mismatch, non-null `prev` at seq 1. |
| `canonical_json` | 4 | Values and their canonical encodings, including the RFC 8785 key-sorting example. |
| `product_keys` | 5 | Fields, root ID, and derived key. |

Seeds are `sha256("perch test seed <n>")`. These keys are public. Never use them for real data.

## 14. Implementation notes (`perch-core`)

### 14.1 Main types

| Item | Purpose |
|---|---|
| `Record::from_json_str(&str)` | Strict parse plus isolated validation |
| `Record::from_value(Value)` | Isolated validation of an in-memory value (cannot detect duplicate keys or lone surrogates) |
| `sign_record(&Keypair, Unsigned)` | Create a signed record |
| `Record::to_json_string()` | Wire form |
| `Record::without_body()`, `with_body(Map)` | Redaction and body fill |
| `kinds::check_record(&Record)` | Semantic body check, returns a typed `Body` |
| `EvidencePair::new(x, y)` | Fork evidence |
| `sign_receipt`, `verify_receipt` | Receipts |
| `ProductKey::derive`, `kinds::derive_product_key` | Product keys |
| `https_url_host` | ASCII host for domain matching |
| `currency::CURRENCIES` | ISO 4217 table |

All of `perch-core` compiles to `wasm32-unknown-unknown`. `Keypair::generate()` exists only on native targets.

### 14.2 Tests

`cargo test -p perch-core`: 76 tests (69 unit, 5 property, 1 vector check, 1 doctest), plus 2 ignored (vector writer, benchmark).

| Property test | Cases | Checks |
|---|---|---|
| `canonical_is_stable` | 512 | `encode(parse(encode(x))) = encode(x)` and `parse(encode(x)) = x` for random JSON with control, astral, and private-use characters |
| `canonical_ignores_input_form` | 512 | Shuffled keys, random whitespace, optional `\u` and `\/` escapes give the same encoding |
| `wire_roundtrip` | 512 | Wire form is canonical and round-trips; header-only keeps the ID |
| `single_byte_mutation_detected` | 512 | Any single-byte change to a wire string fails to parse, fails verification, or changes the ID |
| `mutation_exhaustive_on_one_record` | 1 record × every byte × 11 values | No mutated record verifies |

### 14.3 Throughput

`cargo test -p perch-core --release --test bench -- --ignored --nocapture`. Machine: 2 shared cores, load average 1.75 during the run. Review records with a 360-character text; mean wire size 807 bytes.

| Operation | Records per second |
|---|---|
| `sign_record` | 19,887 |
| `Record::from_json_str` (parse, canonicalize, hash, verify) | 14,751 (11.9 MB/s) |
| `kinds::check_record` (schema) | 1,263,490 |
| `Record::to_json_string` | 1,221,691 |

Verification dominates. At 14,751 records per second, replaying 1 million records takes about 68 seconds on one core.

## Appendix: terms

| Term | Meaning |
|---|---|
| Canonical JSON | The single byte encoding of a JSON value defined in §3.1. |
| Chain | A record plus all records that replace it, directly or transitively. |
| Control kind | A kind whose body a node must not remove (§5.3). |
| Domain (signature) | A fixed prefix signed before the message, so a signature for one purpose is invalid for another. |
| Ed25519 | An elliptic-curve signature scheme (RFC 8032). |
| Evidence pair | Two records that prove an author signed two histories (§10). |
| GTIN | Global Trade Item Number: the number behind UPC and EAN barcodes. |
| Header-only record | A record whose body was removed or not yet received. |
| IDNA | Internationalized Domain Names in Applications; ToASCII converts a Unicode hostname to its `xn--` form. |
| JCS | JSON Canonicalization Scheme, RFC 8785. |
| JSONL | JSON Lines: one JSON value per line. |
| Minor unit | The smallest unit of a currency, for example the cent. |
| MPN | Manufacturer Part Number. |
| Root ID | ID of the first record in a chain. |
| Scalar value | A Unicode code point other than a surrogate; one Rust `char`. |
| Small-order point | One of 8 curve points `P` with `[8]P` = identity. A public key of small order allows forged signatures. |
| SPKI | SubjectPublicKeyInfo: the DER wrapper OpenSSL uses for public keys. |
| UTF-16 code unit | A 16-bit value; characters above U+FFFF use two (a surrogate pair). |
| Wire form | The canonical JSON of the full record: the stored, relayed, and exported form. |
