# Perch verified purchase

Status: draft for contract version 3 (2026-10-18). Owner: security.

This document compares ways for a buyer to prove a purchase, and recommends a path for v1 and v2. Section numbers such as "4.7" refer to [`ARCHITECTURE.md`](../ARCHITECTURE.md).

## 1. What a purchase proof must show

A review is worth more when it comes from someone who bought the product. A proof should show, for one review:

| # | Claim | Why |
|---|---|---|
| P1 | A purchase happened. | Rules out reviewers who never bought. |
| P2 | It was this product (or this listing). | Stops reuse of one purchase across products. |
| P3 | The buyer is the review's author. | Stops one buyer's proof from being reused by others. |
| P4 | The seller is the one claimed. | Shows which seller the experience concerns. |
| P5 | One purchase supports at most one counted review. | Stops one purchase from backing many reviews. |

A proof also has costs:

| Property | Question |
|---|---|
| Cooperation | Who must take part: seller, mail server, website, payment processor? |
| Forgery resistance | What must an attacker corrupt to fake a proof? |
| Privacy | What does the proof reveal, and to whom? |
| Maturity | Is it standardized, deployed, audited? |
| Cost | What must Perch build, and what does verifying cost a client? |

No proof shows that the purchase was not paid back. A seller can refund a buyer for a 5-star review ([He, Hollenbeck, Proserpio 2022](https://cacm.acm.org/magazines/2023/10/276636-leveraging-social-media-to-buy-fake-reviews/fulltext)), and the proof stays valid. This is why verified purchase also requires verified identities counted once per person (7.5).

## 2. The browser extension cannot be the source of trust

The user proposed a browser extension that captures purchases, and noted that a user can modify the extension's code. This concern is correct, and it decides the design:

- The extension runs on the user's device. The user, or malware with the user's rights, can change its files, its memory, and the pages it reads. Chrome's own integrity checks on profile files can be recomputed by a process running as the user ([Rubrik Zero Labs 2026](https://zerolabs.rubrik.com/blog/inside-ghostchrome-x-chrome-extension-integrity-bypass)).
- Any statement the extension makes ("this user bought X") is therefore a statement by the user. A signature by an "extension key" adds nothing, because the key is on the same device.
- Remote attestation of the browser would require the platform vendor to vouch for the software state. Google withdrew its proposal for this on the web, the Web Environment Integrity API, in 2023 ([The Register](https://www.theregister.com/2023/11/02/google_abandons_web_environment_integrity)). Remote attestation would also exclude users of open browsers.

Rule: **the extension may only collect evidence that a third party signed.** That third party can be the seller (a receipt), the seller's mail server (a DKIM signature), or the seller's web server (a TLS transcript certified by a notary). Views verify that evidence. The extension is a courier, never a witness.

## 3. Options

### 3.1 Seller-signed receipts (v1)

The seller signs a detached receipt for the buyer's key (4.7):

```text
Receipt { listing, buyer, purchased_at, nonce, sig }
sig = Ed25519(seller key, "perch/receipt/v1\n" || canonical_json({ listing, buyer, purchased_at, nonce }))
```

- **Proves:** the seller of the listing states that `buyer` bought from `listing` at `purchased_at`. Covers P1 (as a seller claim), P2, P3, and P4. P5 holds because views count only the latest review per (author, product).
- **Cooperation:** the seller must issue receipts. The buyer must give the seller a Perch key at checkout.
- **Forgery resistance:** an outsider cannot forge a receipt without the seller's key. The seller can forge any number of receipts for its own sybils (threat T25). Contract 7.5 limits this: the seller and, by default, the buyer must be verified, counted once per person.
- **Privacy:** the receipt stays with the buyer until the buyer cites it in a review. Then it reveals the listing, purchase time, and buyer key. It does not reveal the order number, price paid, or address.
- **Maturity:** uses only Ed25519, which v1 already ships.
- **Cost:** small. `issueReceipt` and `verifyReceipt` in `PerchClient` (8.2).

Weakness: the seller chooses who gets receipts (threat-model R8). A seller can skip buyers who complain. Mitigations:

1. Issue at checkout, before the outcome is known. A seller plugin issues a receipt for every order that carries a Perch key.
2. Views show the share of a seller's reviews that cite receipts, next to verified-purchase ratings, so missing receipts are visible.
3. Add proofs that do not need the seller (sections 3.2 to 3.4).

### 3.2 DKIM-signed receipt emails

Most shops send an order confirmation email. The sending domain signs it with [DKIM (RFC 6376)](https://www.rfc-editor.org/rfc/rfc6376). The signature covers selected headers (usually `From`, `To`, `Subject`, `Date`) and a hash of the body. Anyone can verify it with the public key that the domain publishes in DNS.

- **Proves:** a message with this body was sent through the mail servers of the signing domain. Covers P1 and P4 (the domain), and P2 if the body names the product.
- **P3 needs extra work.** The signature binds the `To` header, not a Perch key. The buyer must show that the `To` address is theirs. Options: (a) reveal the address, losing privacy; (b) the identity attestor attests a hash of the buyer's email address; (c) prove it in zero knowledge.
- **Cooperation:** none from the seller beyond its normal email.
- **Forgery resistance:**
  - Rests on the domain's DKIM key. In 2024, researchers factored a 512-bit DKIM key for about €8 of cloud time. Of about 475,000 popular domains with DKIM, almost half used keys of 1,024 bits or less ([heise 2025](https://heise.de/-10243244)). [RFC 8301](https://www.rfc-editor.org/rfc/rfc8301) forbids keys under 1,024 bits. Perch should accept only 2,048-bit keys or larger.
  - Anyone who controls the domain's mail system can sign anything. For marketplaces, the marketplace signs, not the third-party seller.
  - DKIM keys rotate. Verifying an old email needs the old public key, which DNS no longer serves. The [DKIM archive of the zk-email project](https://website-prove-email-pr-15.onrender.com/blog/archive) holds about 1 million selector-domain keys, but it is one more party to trust.
- **Privacy:** the plain email reveals order number, price, name, and address. Showing it publicly is unacceptable. Use zk-email (section 3.3).
- **Replay:** one confirmation email can be shown many times. P5 needs a pseudonym derived from the email's signature, so a second use is detected.
- **Maturity:** DKIM is universal. Templates differ per shop, so parsing the product needs per-shop rules.
- **Cost:** RSA-2048 verify in views (cheap); per-shop template rules; DKIM key archive records.

### 3.3 zk-email

[zk-email](https://docs.zk.email/) proves in zero knowledge that a DKIM-signed email matches a pattern, for example "From: orders@shop.example; body contains 'Product: X'", and reveals only chosen parts.

- **Proves:** the same as 3.2, with only selected fields revealed. The proof can output a value derived from the email's signature, which works as a one-use pseudonym for P5.
- **Measured cost** ([PoPETs 2026](https://petsymposium.org/popets/2026/popets-2026-0143.php), Apple M4 Pro laptop):

  | Backend | Proof size | Prove time | Verify time |
  |---|---|---|---|
  | Groth16 (Circom) | 805 bytes | 6.9–33.0 s | ≈188 ms |
  | UltraHonk (Noir) | 14,080 bytes | 3.8–15.1 s | 1.7–5.8 s |

  Circuit size grows with email size: about 545 constraints per byte in Circom and 87 gates per byte in Noir. SHA-256 dominates.
- **Trust:** the same DKIM and key-archive assumptions as 3.2. Groth16 also needs a trusted setup per circuit.
- **Maturity:** active research and audited components. Each shop's email template needs its own pattern and circuit, so coverage grows shop by shop.
- **Cost to Perch:** high. A circuit per template, a proving flow in the browser or extension, a wasm verifier in views, and a DKIM key registry as records.

### 3.4 TLS-based proofs (TLSNotary, zkTLS)

The buyer opens the shop's order-history page. A protocol produces a proof that this content came from the shop's server over TLS.

| System | How | Trust | Measured cost |
|---|---|---|---|
| [TLSNotary](https://tlsnotary.org/docs/intro), MPC mode | The prover and a notary jointly run the TLS client with multi-party computation (MPC). The notary signs commitments to the transcript. | The notary must not collude with the prover. Several notaries reduce this risk. | 1 KB request and 2 KB response: 4.0 s native and 6.2 s in the browser at 100 Mbps and 15 ms latency; 17.0 s in the browser at 20 Mbps ([alpha.14 benchmarks](https://tlsnotary.org/blog/2026/01/19/alpha14-performance)). TLS 1.2 only ([FAQ](https://tlsnotary.org/docs/faq)). |
| TLSNotary, [proxy mode](https://tlsnotary.org/blog/2026/04/22/proxy-mode) | The verifier relays the TLS traffic and checks a zero-knowledge proof about it. | No attacker may intercept the path from verifier to server. The server sees the verifier's IP address. | Described as faster than MPC; benchmarks not yet published. |
| Proxy-based zkTLS ([Reclaim Protocol](https://blog.reclaimprotocol.org/posts/proxying-is-enough)) | Same idea as proxy mode, with an attestor network. | The attestor is honest and the network path is not attacked. | Not published in the cited post. |
| [DECO](https://arxiv.org/abs/1909.00938) | Three-party handshake with zero-knowledge proofs. | Research design. | — |

- **Proves:** the shop's server sent this page in a session. Covers P1, P2, and P4 if the page shows the order. P3 needs the page to show something tied to the buyer's key, or an attested account.
- **Cooperation:** none from the seller.
- **Forgery resistance:** depends on the notary or proxy. A seller can run its own notary and certify fake pages, so a policy must list trusted notaries, as it lists attestors.
- **Privacy:** the prover reveals only chosen parts. The notary learns the server name.
- **Maturity:** alpha software; TLS 1.2 only for MPC mode; HTML layouts change often.
- **Cost:** high. Notary infrastructure, per-site page parsers, verifier code in views.

### 3.5 Payment-processor attestations

A card network or payment provider signs "this buyer paid this merchant on this date".

- **Proves:** P1 and P4 strongly. P2 only if the merchant passes item data to the processor, which is uncommon.
- **Cooperation:** the payment provider, and usually the merchant.
- **Forgery resistance:** strong, if the provider's key is safe.
- **Privacy:** the provider learns that the buyer uses Perch. The attestation can carry only a merchant identifier and a date.
- **Maturity:** no general public API offers buyer-held, publicly verifiable payment attestations today. Our searches found none.
- **Cost:** low for Perch once a provider offers it; in practice depends on partnerships.

### 3.6 Browser-extension capture

The extension reads the order page or confirmation email in the buyer's browser.

- **Proves:** nothing by itself (section 2).
- **Useful role:** courier. It can capture the seller receipt at checkout (3.1), forward a DKIM email to the zk-email prover (3.3), or run a TLSNotary session (3.4). In each case a third party signed the evidence.

### 3.7 Comparison

| Option | P1 | P2 | P3 | P4 | P5 | Seller cooperation | Forged by | Privacy | Maturity | Perch cost |
|---|---|---|---|---|---|---|---|---|---|---|
| Seller receipt | Seller claim | Yes | Yes | Yes | Per-author rule | Required | Seller | Good | Shipped | Low |
| DKIM email, plain | Yes | Template | Needs email binding | Domain | Needs pseudonym | None | Domain operator, weak keys | Poor | Universal | Medium |
| zk-email | Yes | Template | Needs email binding | Domain | Proof output | None | Domain operator, weak keys | Good | Research, audited parts | High |
| TLSNotary / zkTLS | Yes | Page | Needs account binding | Server | Needs pseudonym | None | Notary colluding with buyer | Good | Alpha | High |
| Payment attestation | Yes | Rarely | Yes | Merchant | Per payment | Often | Provider | Medium | Not available | Low, if offered |
| Extension capture alone | No | No | No | No | No | None | Anyone | — | — | Low |

## 4. Recommendation

### 4.1 v1

1. Ship seller-signed detached receipts (4.7, 7.5).
2. Count a review as a verified purchase only with a verified seller and a verified buyer, counted once per person (7.5).
3. Provide a seller plugin and checkout flow that issues a receipt for every order carrying a Perch key. Receipts are issued before any review exists.
4. The extension may capture a receipt from the checkout page and store it for the buyer. It must never create a receipt.
5. Views show, per seller, the number of reviews with and without receipts.

### 4.2 v2

1. Add an evidence kind for DKIM-based proofs: `review.receipt_proof` with `type: "zk-email"`, a circuit identifier, the proof, and public outputs (domain, product text, one-use pseudonym).
2. Add a DKIM key registry as records by trusted attestors: `attest.dkim { domain, selector, key_sha256, seen_from, seen_until }`. Reject keys below 2,048 bits.
3. Bind the email address to the buyer through the identity attestor: the attestor attests `HMAC(email)` for the buyer's person, and the zk-email proof shows that the `To` address hashes to that value.
4. Re-evaluate TLSNotary once TLS 1.3 is supported and proxy-mode benchmarks are published. If adopted, notaries are listed in the policy like attestors.
5. Combine with anonymous credentials (identity.md section 7): a review then proves "a distinct verified person bought this product" with no public link to the person.

## 5. Threats specific to receipts

| Threat | v1 handling | Test (in `crates/perch-tests/tests/purchase.rs`) |
|---|---|---|
| Receipt signed by someone other than the listing's seller | Not a verified purchase | `receipt_by_non_seller_not_verified` |
| Review citing another buyer's receipt | Not a verified purchase | `review_citing_someone_elses_receipt_not_verified` |
| Receipt for listing of a different product | Not a verified purchase | `receipt_for_other_product_not_verified` |
| Seller reviewing its own product with a receipt to itself | Not a verified purchase | `seller_review_with_own_receipt_not_verified` |
| Unverified seller | Not a verified purchase | `unverified_seller_receipt_not_verified` |
| Modified receipt field | Not a verified purchase | `tampered_receipt_not_verified` |
| Seller retracts the listing to strip status | Status kept | `receipt_survives_listing_retract` |
| Receipt signature reused as a record signature | Rejected by domain separation | `isolated.rs`: `receipt_signature_not_valid_as_record` |

## 6. Anonymous receipts (v2 sketch)

To let buyers review without linking a purchase to their identity, the seller can blind-sign a receipt:

1. At checkout, the buyer's client creates a secret `s` and sends a blinded commitment to `(listing, s)`.
2. The seller blind-signs it with a BBS key (identity.md section 7.3). The seller does not learn `s`.
3. Later, the buyer proves in zero knowledge "I hold a seller-signed receipt for this listing" and outputs the pseudonym `H(s, "perch/review/v1/" + product)`.
4. Views count one review per pseudonym.

The seller cannot link the review to the order, and cannot withhold status after the fact. The seller can still decide whom to give receipts at checkout. This needs BBS blind issuance, which is a separate CFRG draft, and is not in v1.

## Appendix A: terms and acronyms

| Term | Meaning |
|---|---|
| Blind signature | A signature on a message that the signer cannot see. |
| Circuit | The program a zero-knowledge proof system proves facts about. |
| DKIM | DomainKeys Identified Mail: a mail server's signature over a message (RFC 6376). |
| MPC | Multi-party computation: several parties compute a function without revealing their inputs to each other. |
| Notary | In TLSNotary, a party that takes part in the TLS session and signs a commitment to the transcript. |
| Selector | The DKIM name that, with the domain, locates the public key in DNS. |
| TLS | Transport Layer Security, the protocol behind `https://`. |
| Trusted setup | A one-time ceremony creating parameters for some proof systems such as Groth16. |
| zkTLS | Zero-knowledge proofs about data received over TLS. |
| Zero-knowledge proof | A proof that a statement is true which reveals nothing else. |
