# Perch identity

Status: draft for contract version 3 (2026-10-18). Owner: security.

This document explains how Perch ties keys to real people and businesses, how a policy chooses whom to trust for that, how keys are rotated and recovered, and how buyers can become anonymous later. Section numbers such as "7.3" refer to [`ARCHITECTURE.md`](../ARCHITECTURE.md).

## 1. Why identity is needed

Anyone can create an Ed25519 key in under a millisecond. If each key counted as one voice, one person could post a million reviews or flags. [Douceur (2002)](https://www.microsoft.com/en-us/research/publication/the-sybil-attack/) shows that a system without a logically central identity authority cannot prevent this.

Perch does not trust key counts. Flags and ranking count only **verified** authors, once per **person**: a set of keys linked by shared `(attestor, person)` pairs (7.3, 7.6, 7.8). An identity attestor decides who is a distinct person. The policy decides which attestors to believe.

## 2. Keys and authors

- An author is an Ed25519 public key ([RFC 8032](https://www.rfc-editor.org/rfc/rfc8032)) written as 64 lowercase hex characters.
- The secret key is a 32-byte seed. `PerchClient.exportKey()` returns it with the author's latest head `{ seq, id }` (8.2).
- One key has one log. Signing two different records at one seq is a fork, and views stop trusting the log from that seq on (5.2).

A person can have many keys, for example one per device or one per role. Every verified key of one person carries the same `person` pseudonym from a given attestor, so it still counts once.

## 3. Attestors

### 3.1 What an attestor is

An attestor is an author whose `attest.*` records a policy accepts. Nothing in the protocol makes an author an attestor; only a policy does.

| Kind | Claim | Fixed fields |
|---|---|---|
| `attest.identity` | "Key `subject` belongs to real person or business `person`, of level `level`, in `country`, until `expires`." | `subject`, `person` |
| `attest.domain` | "Key `subject` controls `domain` until `expires`." | `subject`, `domain` |
| `attest.revoke` | "Key `subject` was compromised after its record `valid_through_seq`." | — (not replaceable) |

### 3.2 What an identity attestor does

An identity attestor must do these steps for each subject. It publishes its procedure, so users can judge it.

1. Verify the human or business. For a person: a government ID plus a liveness check. Commercial services charge about $1.50 per check ([Stripe Identity](https://stripe.com/identity)). For a business: a registry extract and a check that the applicant may act for it.
2. Verify key control. The applicant signs a challenge from the attestor with the key. The challenge uses its own signature domain, for example `perch/attest-challenge/v1\n` + random 32 bytes, so the signature cannot be reused as a record.
3. Compute `person`. Use `HMAC-SHA-256(attestor_secret, normalized_identity)`, where `normalized_identity` is a stable identifier such as country + document number, or a business registry number. The same input always gives the same `person`, so a second key of the same human gets the same value. Only the attestor can compute it.
4. Publish `attest.identity` with an `expires` of at most 1 year.
5. Store the verification evidence under its own data-protection duties, outside Perch. Never put identity documents in records.

### 3.3 What a domain attestor does

A domain attestor checks that a seller controls a web domain, so that the seller's listings can feed price rankings (7.4).

It accepts either of two proofs (4.5):

| Proof | Location | Content |
|---|---|---|
| Well-known file | `https://<domain>/.well-known/perch.json` | `{ "perch": 1, "authors": ["<subject hex>"] }` |
| DNS TXT record | `_perch.<domain>` | `perch-author=<subject hex>` |

The attestor:

1. Fetches the proof over HTTPS with a valid certificate, or over DNS. It should query DNS from at least 2 network locations to reduce the risk of a spoofed answer.
2. Checks that `subject` appears exactly as 64 lowercase hex characters.
3. Publishes `attest.domain { subject, domain, expires }` with `expires` at most 90 days ahead. Short expiry limits the damage when a domain changes owners.
4. Re-checks before renewal. If the proof is gone, it lets the attestation expire or retracts it.

### 3.4 Seller steps for domain verification

These are the steps a seller takes. Replace `shop.example` with your domain and `<hex>` with your Perch author key (shown in the app under your profile).

1. **Choose one method.** Use the well-known file if you can upload files to your web server. Use DNS if you manage your domain's DNS records.

2. **Well-known file method.**
   1. Create a file named `perch.json` with this exact content:

      ```json
      { "perch": 1, "authors": ["<hex>"] }
      ```

      To list several keys (for example a second shop key), add them to the array: `["<hex1>", "<hex2>"]`.
   2. Upload it so that it is served at `https://shop.example/.well-known/perch.json`.
   3. Check it from a terminal:

      ```sh
      curl -s https://shop.example/.well-known/perch.json
      ```

      The output must be the JSON above. The server must use HTTPS with a valid certificate and must not redirect to another host.

3. **DNS method.**
   1. In your DNS provider, add a TXT record:

      | Name | Type | Value |
      |---|---|---|
      | `_perch.shop.example` | TXT | `perch-author=<hex>` |

      For several keys, add one TXT record per key.
   2. Wait for the record to propagate. Check it:

      ```sh
      dig +short TXT _perch.shop.example
      ```

      The output must include `"perch-author=<hex>"`.

4. **Request verification** from a domain attestor listed in the policy your customers use. The official app shows the default attestors under Settings → Policy. Give the attestor your domain and author key.

5. **Confirm.** After the attestor publishes `attest.domain`, your listings whose `url` host is `shop.example` or a subdomain such as `www.shop.example` show `domain_verified: true`.

6. **Keep the proof in place.** The attestation expires (at most 90 days by the procedure above). The attestor re-checks before renewing. If you remove the file or the TXT record, verification ends at expiry.

7. **Change keys.** Add the new key to the file or DNS first, request a new attestation, then remove the old key.

Listings on marketplace domains such as amazon.com cannot be domain verified by the seller. See [`threat-model.md`](threat-model.md) R5 for the proposed alternatives.

### 3.5 Attestor duties and conflicts

- An attestor that is also a seller should not attest itself. Views do not forbid this, but users can see it, because the attestor's and subject's keys are public.
- An attestor must not attest a key it cannot tie to a person. If it does, every policy that trusts it inherits the sybils.

## 4. How a policy selects attestors

The policy lists attestors and what each is trusted for (7.1):

```json
"trusted_attestors": [
  { "author": "<attestor A>", "kinds": ["attest.identity", "attest.revoke"], "not_after_seq": null },
  { "author": "<attestor B>", "kinds": ["attest.domain"], "not_after_seq": 1200 }
]
```

| Field | Meaning |
|---|---|
| `author` | The attestor's key. |
| `kinds` | Which attestation kinds count from this attestor. |
| `not_after_seq` | If set, only this attestor's records with `seq ≤ not_after_seq` count. Used after a key compromise. |

An attestation counts when all of these hold (7.3): its author is trusted for its kind; it is within `not_after_seq`; it is below the attestor's fork seq and revoke cutoff; it is the latest version of its chain; it is not retracted; and `expires > policy.now`.

### 4.1 Choosing attestors for a default policy

| Criterion | Reason |
|---|---|
| Published verification procedure | Users can judge what "verified" means. |
| Independent organizations, at least 2 | One compromise or outage does not remove all verification (threat-model R1). |
| Key in a hardware security module | Raises the cost of key theft. |
| Published successor key | Allows recovery after a fork of the attestor's own log. |
| Jurisdiction and data-protection compliance | Attestors process identity documents. |

Each extra identity attestor lets one human count once more (threat-model R3). The default should list few identity attestors.

### 4.2 Users and forks of the policy

The app ships its default policy inside the signed release and shows its hash (7.1, 10). A user can add or remove attestors. A fork of the app can ship a different default. Because views run on the client, no node can change which attestors a user trusts.

## 5. Bootstrapping the first attestors

At launch no attestor exists. The options:

| Option | How | Strength | Weakness |
|---|---|---|---|
| A. Project-run attestor | The Perch maintainers run one attestor using a commercial ID-check service. | Fast to start. | One organization decides who is verified. |
| B. Existing verified identities | Accept proofs from systems that already verify people, for example a national eID or a bank, through an attestor that bridges them. | Reuses strong checks. | Integration work per system; privacy depends on the source. |
| C. Web of trust | Verified people vouch for others. | No central party. | Easy to farm: one corrupt member can vouch for many sybils. |
| D. Business registries | For sellers, check public company registries. | Public, auditable data. | Covers businesses only. |

Recommendation for v1:

1. Start with option A for persons and option D for businesses, run by 2 separate legal entities with separate keys.
2. Publish both procedures and both keys in the release.
3. Invite independent organizations (consumer groups, payment providers) to run attestors, and add them to the default policy once they publish a procedure.
4. Do not accept web-of-trust vouches for sybil resistance.

## 6. Expiry, revocation, and key loss

### 6.1 Expiry

- `attest.identity.expires` and `attest.domain.expires` are required. Views drop the attestation when `expires ≤ policy.now`.
- When an attestation lapses, the author's past flags and verified reviews stop counting (7.3 evaluates at `policy.now`). Attestors should renew before expiry.

### 6.2 Retraction by the attestor

An attestor retracts an attestation with `retract`. Use this when the subject asks to leave, or when verification was wrong. Retraction is final for that chain (4.6). The attestor can issue a new attestation chain later.

### 6.3 Key compromise of a user

1. The user tells the attestor and proves identity again.
2. The attestor publishes `attest.revoke { subject: <old key>, valid_through_seq: <last seq the user wrote> }`.
3. Views ignore the old key's records after that seq and mark it revoked (7.3).
4. The user creates a new key and gets a new `attest.identity` with the same `person`.

A thief may fork the stolen key. Per 7.3, a fork on a key that has a counting revoke does not restrict the person. Until the revoke is published, the person is restricted (threat-model R7).

### 6.4 Key compromise of an attestor

1. The attestor tells policy maintainers and users the last seq it signed honestly.
2. Policies set `not_after_seq` to that seq. Records after it stop counting.
3. If the thief forks the attestor's log at seq k, everything from seq k on stops counting automatically (5.2). If k = 1, all of the attestor's attestations stop counting (threat-model R1).
4. The attestor publishes attestations again under its successor key, and policies add that key.

### 6.5 Key loss without compromise

1. The user creates a new key.
2. The attestor issues `attest.identity` for the new key with the same `person`.
3. Optionally the user publishes `retire { successor }` from the old key if a backup survives. `retire` transfers nothing (4.5); it is informational.

Old reviews by the lost key stay valid and still count once for that person. Because of "latest review per (author, product)" and per-person counting, the person cannot double-count a product with the new key.

### 6.6 Key rotation

Rotation is the same as section 6.5 with a planned `retire`. The old key's records remain valid. Per-person status (restriction) follows the person to the new key (7.3).

### 6.7 Several devices

v1 has no subkeys. Two devices that share one key will fork the log if both publish at the same seq. Contract section 10 requires a client to check its own head before publishing. For two active devices, use two keys verified with the same `person`.

## 7. Path to anonymous buyers

### 7.1 Problem

In v1, a verified buyer's reviews are public and signed by a key that an attestor tied to a real person. The `person` pseudonym also groups all of that person's keys. Anyone can read what a verified person reviewed, and the attestor can name them.

### 7.2 Goal

A buyer proves "I am a distinct verified person" and "I bought this product", without revealing who they are, while still counting once per product. Precisely:

| Property | Meaning |
|---|---|
| Sybil resistance | One verified person yields one review or flag per context (for example per product). |
| Unlinkability | Reviews by one person on different products cannot be linked, by the public or by the attestor. |
| Public verifiability | Any client can check the proof with public data only. Views must stay pure functions of public records (D5). |
| Revocability | A credential can stop working after expiry or abuse. |

The common mechanism is a **context pseudonym** (also called a nullifier): a value derived from the person's secret and a context string, such as `"perch/review/v1/" + product_key`. The same person and context always give the same value, so views count it once. Different contexts give unlinkable values.

### 7.3 Candidate schemes

| Scheme | How it works | Proof size | Verify cost | Public verification | Maturity |
|---|---|---|---|---|---|
| [BBS signatures](https://datatracker.ietf.org/doc/draft-irtf-cfrg-bbs-signatures/) with [per-verifier linkability](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-bbs-per-verifier-linkability-03) | Attestor signs a credential containing a secret. The holder derives a zero-knowledge proof of the signature and a pseudonym per context. | 3 G1 points + (4 + U) scalars: 272 + 32·U bytes on BLS12-381, U = hidden attributes; plus a 48-byte pseudonym | One BBS proof check ≈ 1.6 ms native ([libbbs benchmark](https://github.com/Fraunhofer-AISEC/libbbs)) | Yes, with the attestor's public key | CFRG draft 13 (2026-10); pseudonym draft 03 (2026-06), sybil section marked TODO; needs a pairing library in wasm |
| [Privacy Pass](https://www.rfc-editor.org/rfc/rfc9576.html) tokens ([RFC 9578](https://www.rfc-editor.org/rfc/rfc9578.html)) | Issuer blind-signs one-time tokens after attestation. | 146 bytes (VOPRF) or 354 bytes (Blind RSA) | One RSA-PSS verify for Blind RSA | Only Blind RSA (type 0x0002); VOPRF needs the issuer's secret key | RFCs since 2024; deployed at scale (Apple Private Access Tokens) |
| Semaphore ([v4](https://docs.semaphore.pse.dev/)) | Members join a Merkle tree of identity commitments. A Groth16 proof shows membership and a nullifier per scope. | 8 field elements (≈256 bytes) plus root and nullifier | One Groth16 verify, constant time for any circuit; 188 ms measured for zk-email's Groth16 proofs on an Apple M4 Pro ([PoPETs 2026](https://petsymposium.org/popets/2026/popets-2026-0143.php)) | Yes; needs the current group root | Audited v4; trusted setup ceremony for depths 1–32; group tree must be public and synced |
| ZK over existing ID credentials ([Longfellow, Frigo and shelat](https://eprint.iacr.org/2024/2010)) | Prove in zero knowledge that an ECDSA-signed government credential (ISO mdoc) is valid. | Not reported in the paper; larger than BBS (hash-based) | Not reported; ECDSA proof of possession 10–23 ms verify on Apple M4 ([IETF 124 slides](https://datatracker.ietf.org/meeting/124/materials/slides-124-cfrg-longfellow-zk-01)) | Yes | Open source since 2025-07 ([Google](https://blog.google/technology/safety-security/opening-up-zero-knowledge-proof-technology-to-promote-privacy-in-age-assurance/)); mdoc proof 1.2 s on a phone; no Perch-ready pseudonym output yet |

Notes on each:

- **BBS.** Gives everything in section 7.2 natively. The EU digital identity wallet did not adopt it, because the scheme is not on the approved list and phone secure elements only support ECDSA ([Lehmann, RWC 2025](https://iacr.org/submit/files/slides/2025/rwc/rwc2025/95/95_slides.pdf)). That barrier does not apply to Perch, which runs its own attestors.
- **Privacy Pass.** Tokens are one-time and unlinkable but carry no context pseudonym. A person could get N tokens and post N reviews of one product. Rate limiting depends on the issuer. The [Anonymous Rate-Limited Credentials draft](https://datatracker.ietf.org/doc/draft-ietf-privacypass-arc-crypto/01/) would add limits, but it is expired and is not publicly verifiable.
- **Semaphore.** Gives context nullifiers today. It needs a public, agreed group root. In Perch, the attestor would publish the tree root as a record. Root updates would need to be in views' inputs.
- **Longfellow.** Lets buyers use credentials they already have, which removes the identity attestor. It needs a circuit that outputs a context pseudonym from a stable credential field. That circuit does not exist yet.

### 7.4 Proposed path

| Step | Content | Contract impact |
|---|---|---|
| v1 (now) | Real-world attestations; public `person` pseudonym | None |
| v2 | New kind `review.anon`, authored by a fresh one-time key. The body carries a BBS proof from a trusted attestor, a pseudonym for context `"perch/review/v1/" + product`, and optionally an anonymous receipt (verified-purchase.md section 6). Views count one review per (attestor, pseudonym). | New kind, new signature domain `perch/anon-credential/v1\n`, policy field `anon_attestors` |
| v2 | Same mechanism for flags: context `"perch/flag/v1/" + target root` | New kind `flag.anon` |
| Later | Evaluate Longfellow-style proofs over government credentials, to remove the identity attestor from the trust path | Policy lists credential issuers instead of attestors |

Before adopting BBS:

1. Wait for the pseudonym draft's sybil section to be complete.
2. Pick a Rust implementation that compiles to wasm and has been audited.
3. Measure proof generation and verification in the browser. The target is under 50 ms verification per review, so that a product with 1,000 anonymous reviews verifies in under 1 minute on first load. Cache results by record ID.

### 7.5 Tradeoffs

| Topic | v1 (identity attestation) | v2 (anonymous credential) |
|---|---|---|
| Added bytes per review | 0 | 672 hex characters for a BBS proof with U = 2, plus 96 for the pseudonym |
| Verify cost per review | One Ed25519 verify: 44.5 µs native (`threat-model.md` section 9.1) | One BBS proof check (1.6 ms native in libbbs; wasm not measured) |
| Linkability | Public `person` links all keys | Unlinkable across contexts |
| Attestor knowledge | Knows which key is whose | Knows who holds a credential, not which reviews they wrote |
| Revocation | `attest.revoke` per key | Short expiry plus re-issuance; revocation lists or accumulators add size |
| Abuse handling | Restriction follows the person | Restriction can follow only the pseudonym within one context |

The last row is the main cost. With unlinkable reviews, a person who posts fraudulent reviews on one product cannot be restricted on another. Policies can respond by giving anonymous reviews less weight, or by requiring an anonymous receipt for them to count.

## 8. Privacy of attestation records

| Field | Exposure | Mitigation |
|---|---|---|
| `subject` | Public by design | — |
| `person` | Links all keys of one person at one attestor; personal data under GDPR because the attestor can re-identify it | HMAC with an attestor secret, so outsiders cannot compute it from known identity data |
| `level`, `country` | Coarse | Optional `country` |
| `expires` | Reveals renewal habits | Round to the day |

## Appendix A: terms and acronyms

| Term | Meaning |
|---|---|
| BBS | A signature scheme (Boneh, Boyen, Shacham) that allows zero-knowledge proofs of a signature with selective disclosure. |
| BLS12-381 | A pairing-friendly elliptic curve used by BBS. |
| Context pseudonym, nullifier | A value derived from a secret and a context, the same each time for one person in one context, and unlinkable across contexts. |
| eID | Electronic identity issued by a government. |
| G1 point | An element of the first group of a pairing; 48 bytes compressed on BLS12-381. |
| Groth16 | A zero-knowledge proof system with constant-size proofs that needs a trusted setup. |
| HMAC | Hash-based message authentication code ([RFC 2104](https://www.rfc-editor.org/rfc/rfc2104)). |
| Liveness check | A test that a live person, not a photo, is present during identity verification. |
| mdoc | Mobile document format of ISO/IEC 18013-5, used for mobile driving licences. |
| Merkle tree | A hash tree; a short proof shows that a leaf is in it. |
| Person | A connected component of keys linked by shared `(attestor, person)` pairs; counted once (contract 7.3). |
| Person key | The lowest `(attestor, person)` pair in a person; used to identify and sort persons. |
| Privacy Pass | IETF protocol for anonymous tokens (RFC 9576–9578). |
| Trusted setup | A one-time ceremony to create parameters for some proof systems; if all participants collude, proofs can be forged. |
| VOPRF | Verifiable oblivious pseudorandom function. |
