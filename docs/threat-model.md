# Perch threat model

Status: draft for contract version 3 (2026-10-18). Owner: security.

This document lists what Perch protects, who attacks it, where trust changes hands, and what stops each attack. [`ARCHITECTURE.md`](../ARCHITECTURE.md) is the contract; this file explains why it is shaped as it is. Section numbers such as "4.6" refer to `ARCHITECTURE.md`.

Related documents:

- [`identity.md`](identity.md): attestors, keys, and anonymous buyers.
- [`verified-purchase.md`](verified-purchase.md): purchase proofs.
- [`moderation.md`](moderation.md): flags, hiding, appeals, and restriction.

## 1. Scope and method

In scope: record format, cryptography, logs, sync, views, node, web app, extension, attestors, and the default policy.

Out of scope: the security of retailer sites, payment systems, and users' operating systems.

Each threat has a likelihood and an impact.

| Rating | Likelihood | Impact |
|---|---|---|
| High | Expected within the first year of public operation if not mitigated | Breaks a guarantee in section 1 of the contract, or causes users financial loss |
| Medium | Needs a motivated attacker with moderate resources | Distorts views for some products or sellers |
| Low | Needs rare capability or brings the attacker little gain | Nuisance; easy to detect and undo |

"v1" means shipped in contract version 3. "Planned" means proposed for a later version.

## 2. Assets

| ID | Asset | Why it matters |
|---|---|---|
| A1 | Author secret keys, above all attestor keys | A stolen key signs as its owner. A stolen attestor key creates verified identities. |
| A2 | Integrity of records and logs | All views derive from records. |
| A3 | Correctness of views: ratings, rankings, moderation status, verification badges | This is what consumers act on. |
| A4 | Completeness of the data a client receives | A client cannot see hidden fraud warnings or negative reviews it never receives. |
| A5 | Buyer privacy: purchases, browsing, IP address, real identity | Purchases can reveal health, religion, or sexuality. |
| A6 | Seller reputation | False flags and fake negative reviews harm honest sellers. |
| A7 | Client code integrity (wasm and JavaScript) | The client verifies records, computes views, and holds keys. |
| A8 | Node resources: disk, memory, bandwidth, CPU | Exhausting them denies service. |
| A9 | Legal standing of node operators | Operators who cannot meet legal duties will stop running nodes. |

## 3. Adversaries

| ID | Adversary | Goal | Typical capability |
|---|---|---|---|
| E1 | Dishonest seller | Raise own ratings; bury negative reviews; lead price rankings | Many keys; can issue receipts; can pay for reviews |
| E2 | Competitor | Lower a rival's ratings; hide a rival's listings | Same as E1 |
| E3 | Review or flag farm | Sell reviews or flags | Paid crowds. One study observed about 23 active fake-review groups per day on Facebook, each with about 16,000 members ([He, Hollenbeck, Proserpio 2022](https://cacm.acm.org/magazines/2023/10/276636-leveraging-social-media-to-buy-fake-reviews/fulltext)). |
| E4 | Malicious node operator | Censor, eclipse, surveil, or serve altered app code | Controls one or more nodes and the bytes they serve |
| E5 | Network attacker | Same as E4 on the path | Can intercept unencrypted traffic and block connections |
| E6 | Corrupt or compromised attestor | Mint verified identities; revoke honest users | Holds an attestor key |
| E7 | Key thief | Act as a user; destroy a user's log | Malware, phishing, stolen backups |
| E8 | Spammer | Exhaust resources; flood search | Scripts; unlimited free keys |
| E9 | Mob | Brigade a seller or product for non-commercial reasons | Many real, possibly verified people |
| E10 | Content abuser | Publish illegal content or personal data about others | Free keys |
| E11 | Data harvester | Link keys to people and build purchase profiles | Reads the public dataset; may run nodes |

[Douceur (2002)](https://www.microsoft.com/en-us/research/publication/the-sybil-attack/) shows that without a logically central identity authority, one entity can always present many identities. Perch therefore relies on identity attestors (E6 is the cost) and never on counts of free keys.

## 4. Trust boundaries

| Component | Trusts | Does not trust | Boundary enforced by |
|---|---|---|---|
| `perch-core` | The Ed25519 and SHA-256 implementations | Every input byte | Strict parsing, size and depth limits, strict signature verification (4.2) |
| `perch-store` | `perch-core` validation | Arrival order; peers | Complete-prefix rule, deterministic fork handling (5.1, 5.2) |
| `perch-sync` | Store | Peers: content, order, completeness, rate | Re-validation of every record; caps (9); body fetch (6) |
| `perch-views` | Store contents; the policy | Every author, including attestors outside the policy | Semantic rules (4.6, 7.3 to 7.6); `policy.now` instead of a clock |
| `perch-node` | Its operator's configuration | Clients, peers, submitted records | Admission policy (9); no secret keys held (8.1) |
| `perch-wasm` and web app | The release build that a user can check by hash | The node that served it; node API view results | Displayed wasm hash; bundled policy; local views (2.1, 10) |
| Extension | Its signed store release | The retailer page; node API results | No keys held; hash-prefix lookups; node-labeled results (10) |
| Attestors | Their own verification process | Subjects' claims | Their published procedure; HSM-held keys (recommended) |
| Default policy authors | — | — | Policy hash shown to users; users can change the policy (7.1) |

The weakest boundary is between the user and the code that the browser runs. If a node serves the app, the node controls that code. Section 2.1 of the contract handles this with reproducible builds and a displayed wasm hash. The defense only works if someone compares the hash, so signed static releases and the extension are the primary distribution.

## 5. Threats

### 5.1 Records and cryptography

| ID | Threat | Adv. | L | I | v1 mitigation | Planned |
|---|---|---|---|---|---|---|
| T1 | Alter a record's body, author, seq, or signature | E4, E5 | H | H | `body_hash` in the signed header; ID = SHA-256 of the header; Ed25519 signature (4.2) | — |
| T2 | Two encoders produce different bytes for one record, so IDs and signatures disagree | — | M | H | JCS key order by UTF-16 code units; integer-only numbers; one escape rule; normalize all valid input (4.2) | Cross-implementation test vectors |
| T3 | Parser differentials: duplicate keys, `1.0`, `-0`, lone surrogates, uppercase hex, `replaces: null` | E8 | M | H | All rejected at isolated level (4.1, 4.2) | — |
| T4 | Weak or small-order Ed25519 keys make one signature valid for many messages. The identity-point key `0100…00` with signature R = identity, S = 0 verifies for every message under cofactorless checks without small-order rejection. | E1 | M | H | Strict verification: canonical `A` and `R`, no small order, `S < L` (4.2). See [Chalkias et al. 2020](https://eprint.iacr.org/2020/1244). perch-core rejects the vector. | Test vector `identity_point_forgery` in `docs/test-vectors.json` (R14) |
| T5 | Cross-protocol signature reuse: a receipt signature passes as a record signature | E1 | L | M | Domain prefixes `perch/record/v1\n` and `perch/receipt/v1\n` (4.3) | — |
| T6 | Oversized or deeply nested JSON exhausts memory or stack | E8 | H | M | 131,072-byte line cap before parsing; depth ≤ 16; header ≤ 2,048 bytes; body ≤ 65,536 bytes (9) | — |

### 5.2 Logs and history

| ID | Threat | Adv. | L | I | v1 mitigation | Planned |
|---|---|---|---|---|---|---|
| T7 | Author rewrites history by signing a second record at an existing seq | E1, E7 | M | H | Fork evidence; log truncated at the fork seq; author marked forked; per-person restriction (5.2, 7.3) | — |
| T8 | Nodes keep different fork branches and compute different views | E4 | M | H | Truncation at the lowest fork seq; one canonical evidence pair with a total order (5.2) | — |
| T9 | Backdated `ts` fakes a price history ("was €99") | E1 | H | M | `ts` non-decreasing per log; future `ts` ignored (4.1, 7.2) | Witness timestamps (section 7, R2) |
| T10 | Replayed records inflate counts | E4, E8 | H | L | Records deduplicated by ID; one record per (author, seq) | — |
| T11 | A replacement sheds flags or moves reviews to another product | E1 | H | H | References resolve to root IDs; fixed fields (4.6, 4.5) | — |
| T12 | `replaces` or `retract` aimed at another author's record or another kind | E1, E2 | M | H | Same author, same kind, lower seq; retract only by the chain's author; retraction is final (4.6) | — |

### 5.3 Sync and nodes

| ID | Threat | Adv. | L | I | v1 mitigation | Planned |
|---|---|---|---|---|---|---|
| T13 | Eclipse: one node hides records, fork evidence, or log tails from a client ([Heilman et al. 2015](https://www.usenix.org/conference/usenixsecurity15/technical-sessions/presentation/heilman)) | E4 | M | H | Client connects to ≥ 3 nodes when available and shows head disagreements (2.1, 10) | Signed head checkpoints from many nodes |
| T14 | A peer strips bodies before relaying | E4 | M | M | Header-only records relayed; `want_bodies` fetch from other peers (6) | — |
| T15 | Flood of authors or records exhausts storage; held gap records fill memory | E8 | H | M | Per-node admission policy; caps on held records and messages (9) | Admission defaults in `sync-protocol.md` |
| T15a | Sybil storage flood: many free keys, each within the per-log limits A1 and A2 | E8 | H | H | Admission tiers with a global budget for unverified authors (contract 9; R13) | — |
| T16 | Peer table poisoning: one operator fills a node's peer list | E4 | M | M | Capped and diverse address table (6) | — |
| T17 | Self-fork: one node hides a user's last record, so the user publishes at an existing seq and forks | E4 | M | H | Publish guard from local head, backup head, and all nodes (8.2, 10) | — |
| T18 | Node API returns altered view results to third-party clients | E4 | M | M | Policy hash header; `?now=` pinning; export for re-computation (8.1) | — |
| T18a | Sparse-store omission: a node returns a related set without the newest version, a retract, a revoke, a flag, or fork evidence (5.5, 8.1) | E4 | M | M | Extension verifies every record, computes locally, and queries ≥ 2 nodes and shows disagreement (10) | Signed head checkpoints let a sparse client prove it has each author's latest seq |

### 5.4 Identity and attestors

| ID | Threat | Adv. | L | I | v1 mitigation | Planned |
|---|---|---|---|---|---|---|
| T19 | One person verifies many keys and votes many times | E1, E3 | H | H | `person` pseudonym; count once per person key (7.3) | Anonymous one-per-context proofs (identity.md) |
| T20 | Attestor key stolen; thief mints identities | E6, E7 | M | H | `not_after_seq` cutoff in the policy; fork cutoff (7.1, 7.3) | Threshold or HSM-held attestor keys |
| T21 | Attestor revokes honest users to silence them | E6 | L | H | Revoke counts only from an attestor that attested that subject, and only within its trust window (7.3) | — |
| T22 | Self-issued or untrusted attestation | E1 | H | H | Only authors listed in `trusted_attestors` for that kind count (7.3) | — |
| T23 | Restricted or forked seller starts over with a new key | E1 | H | H | Restriction per person key; a forked key restricts the person (7.3) | — |
| T24 | Key thief forks a victim's key to get the victim restricted | E7 | M | M | A fork on a revoked key does not count toward per-person status (7.3; R7) | — |

### 5.5 Purchases and reviews

| ID | Threat | Adv. | L | I | v1 mitigation | Planned |
|---|---|---|---|---|---|---|
| T25 | Seller issues receipts to its own sybil keys and posts "verified purchase" reviews | E1 | H | H | Verified purchase requires a verified seller and, by default, a verified buyer, counted once per person (7.5) | Third-party purchase proofs (verified-purchase.md) |
| T26 | Review cites another buyer's receipt | E1, E3 | H | M | Receipt `buyer` must equal the review author (7.5) | — |
| T27 | Receipt from a non-seller, or for another product | E1 | M | M | Receipt must verify under the listing author; listing product must equal review product (7.5) | — |
| T28 | Seller withholds receipts from buyers likely to complain | E1 | M | M | Receipts are private tokens issued at checkout, before the seller knows the outcome (verified-purchase.md) | DKIM or TLS proofs need no seller cooperation |
| T29 | Seller revokes receipts of unhappy reviewers by retracting or re-listing | E1 | H | M | Receipts are detached; validity does not depend on listing status (4.7) | — |
| T30 | Buying reviews from a review farm | E3 | H | M | Ranking uses identity-verified reviews only (7.8); counted once per person | Purchase proofs; anomaly display |

### 5.6 Moderation

| ID | Threat | Adv. | L | I | v1 mitigation | Planned |
|---|---|---|---|---|---|---|
| T31 | Sybil flags from free keys | E2, E8 | H | H | `flags_require_verified`; once per person key (7.6) | — |
| T32 | Brigade: a group of verified people hides a listing or restricts a seller | E2, E9 | M | H | Threshold; restriction needs `seller_min_distinct_flaggers`; hiding is display-only (7.6) | Juries, counter-signals, time decay (moderation.md) |
| T33 | Seller flags own listing to stage an appeal, or flags competitors | E1 | M | L | Flags by the target's author never count (7.6) | — |
| T34 | Flags on flags create hide cycles | E9 | M | M | Hiding only for listing, review, product, profile; display-only (7.6) | — |
| T35 | Popular listings collect false flags at a fixed rate and get hidden | — | H | M | None beyond the threshold (see moderation.md for numbers) | Engagement-relative thresholds |

### 5.7 Clients

| ID | Threat | Adv. | L | I | v1 mitigation | Planned |
|---|---|---|---|---|---|---|
| T36 | Node serves altered app code that skews views or steals keys | E4 | M | H | Reproducible builds; displayed wasm hash; signed releases first (2.1) | Subresource integrity for the wasm module |
| T37 | Phishing listing: real product, low price, scam URL | E1 | H | H | Domain attestation; only domain-verified listings feed prices (7.4) | — |
| T38 | URL tricks: `https://shop.example@evil.tld/`, IP hosts, look-alike Unicode hosts | E1 | H | H | WHATWG parsing; no userinfo; no IP hosts; punycode display (4.5) | — |
| T39 | Script injection or bidirectional-text spoofing in review text ([Unicode UAX #9](https://www.unicode.org/reports/tr9/)) | E10 | H | H | `textContent` only; `dir="auto"` or `<bdi>` (10) | — |
| T40 | Image URLs reveal viewer IP addresses to a host the author picks | E11 | H | M | No automatic image loading from unverified authors (10) | Content-addressed image relay |
| T41 | Extension reveals browsing to the node | E4, E11 | H | M | 4-hex hash-prefix lookups; local computation over a sparse store (8.1, 5.5) | Prefix length tied to catalog size (R6) |
| T42 | Default policy silently replaced | E4 | M | H | App uses the bundled policy and shows its hash (7.1, 10) | Signed policy records |
| T42a | Key theft from IndexedDB by script in the app's origin: cross-site scripting (XSS), a malicious dependency, or altered code served by a node (D15) | E4, E7 | M | H | `textContent`-only rendering (10); no third-party scripts; confirmed backup. IndexedDB is per origin, so a node can read only keys that were created or imported in the app it serves. | Users keep keys only in the signed-release origin; passphrase encryption (protects offline copies, not live code); non-extractable Web Crypto keys |

### 5.8 Privacy and legal

| ID | Threat | Adv. | L | I | v1 mitigation | Planned |
|---|---|---|---|---|---|---|
| T43 | Public receipts link a real identity to each purchase | E11 | H | H | Receipts are private until the buyer cites one (4.7) | Anonymous verified reviews (identity.md) |
| T44 | `person` pseudonym links all of one human's keys | E11 | H | M | None; documented tradeoff (R4) | Anonymous credentials |
| T45 | Illegal content or personal data in an append-only log | E10 | H | H | Body removal with the signed header kept; `redact` notice (5.3) | — |
| T46 | GDPR erasure requests ([GDPR Art. 17](https://eur-lex.europa.eu/eli/reg/2016/679/oj)) | — | H | H | Bodies removable; headers hold only keys, hashes, and integers (5.3) | Body encryption with per-record keys |
| T47 | Defamation claims against node operators over reviews | E1 | M | M | Body removal on notice; author identity is attested | Notice-handling procedure in `api.md` |

## 6. Guarantees, mechanisms, and tests

Each guarantee in contract section 1 maps to the mechanism that enforces it and the tests that check it. Test paths are relative to `crates/perch-tests/tests/`.

| Guarantee | Mechanism | Tests |
|---|---|---|
| Every claim has a known author | Strict Ed25519 over `perch/record/v1\n` + canonical header (4.2, 4.3) | `isolated.rs`: `valid_record_verifies_with_expected_id`, `universal_forgery_identity_key_rejected`, `tampered_author_rejected`, `tampered_sig_rejected`, `signature_from_other_key_rejected`, `signature_without_domain_prefix_rejected`, `small_order_public_key_rejected`, `non_canonical_s_rejected`, `receipt_signature_not_valid_as_record` |
| No one can alter a claim | ID = SHA-256 of header; `body_hash` in header (4.2) | `isolated.rs`: `tampered_body_rejected`, `body_hash_mismatch_rejected`, `tampered_seq_rejected`, `tampered_ts_and_kind_rejected`, `header_only_record_verifies`; `encoding.rs`: `key_order_and_whitespace_verify`, `escaped_equivalent_strings_verify`, `keys_sorted_by_utf16_not_utf8`, `invalid_encodings_rejected` (26 vectors, each with one defect), `duplicate_key_rejected`, `non_integer_number_tokens_rejected`, `lone_surrogate_rejected`, `uppercase_hex_rejected`, `replaces_null_rejected`, `unknown_top_level_field_rejected`, `unknown_body_fields_preserved`, `unknown_kind_passes_isolated_level`; `core_semantics.rs`: `non_replaceable_kind_with_replaces_is_semantically_invalid`, `body_schema_rejects_bad_values`, `text_lengths_count_scalar_values_not_bytes`, `product_keys_match_contract` (includes the `:` escape collision), `control_and_replaceable_kinds_match_contract`; `limits.rs`: `body_at_limit_accepted`, `oversize_body_rejected`, `oversize_line_rejected`, `depth_16_accepted_depth_17_rejected`, `very_deep_json_does_not_crash`, `wide_body_within_limit_accepted`, `integer_beyond_2_53_rejected`, `ts_beyond_2_53_rejected` |
| No one can silently edit or delete history | Per-author log with `prev`; fork evidence; truncation at fork seq (5.2); reference rules (4.6) | `core_semantics.rs`: `evidence_pair_order_is_total_and_canonical`, `evidence_pair_rejects_other_authors`; `log.rs`: `in_order_log_inserted`, `replayed_record_is_duplicate`, `gap_is_held_not_stored`, `same_seq_fork_detected_and_truncated`, `prev_mismatch_fork_detected`, `fork_state_independent_of_arrival_order` (all 120 orders), `branching_fork_state_independent_of_arrival_order` (120 orders, type-1 and type-2 evidence), `ts_decreasing_conflict_state_independent_of_arrival_order`, `evidence_pair_replaced_by_lower_pair`, `many_forks_keep_one_pair`, `export_replay_reproduces_state`, `sparse_store_accepts_records_without_prefix`, `records_after_fork_not_accepted`, `ts_decrease_rejected`, `equal_ts_accepted`, `invalid_record_never_stored`; `references.rs`: `replacement_updates_price_and_keeps_root`, `replaces_other_author_ignored`, `replaces_other_kind_ignored`, `replacement_changing_fixed_field_ignored`, `replaces_unknown_record_ignored`, `retract_of_other_authors_record_ignored`, `own_retract_retracts_whole_chain`, `retraction_is_final`, `retract_of_control_kind_ignored`, `flags_survive_price_update`, `flag_on_later_version_counts_for_root` |
| No one controls the data | Complete-prefix sync; evidence relay; body fetch; removal list; peer scoring (6, 5.3) | `sync.rs`: `two_nodes_converge`, `both_sides_contribute_and_converge`, `out_of_order_delivery_converges`, `forked_author_from_peer_truncated`, `fork_evidence_spreads_through_a_relay`, `many_equivocations_converge_in_random_graphs` (8 seeds, 12 peers), `invalid_records_not_accepted_from_peer`, `duplicated_messages_do_not_duplicate_records`, `dropped_messages_recovered_on_reconnect`, `stripped_bodies_restored_from_honest_peer`, `removed_body_not_restored_by_sync`, `forged_forked_hint_ignored`, `inflated_head_claim_does_not_block_other_authors`, `unsolicited_push_of_forked_branch_is_evidence_not_history`, `heads_paging_syncs_more_than_one_page`, `synced_nodes_compute_same_views` |
| No one can buy ranking | Views computed locally; ranking from identity-verified reviews only; verified purchase rules; domain verification (7.3–7.5, 7.8) | `core_semantics.rs`: `url_rules_reject_phishing_forms`, `url_hosts_are_ascii_and_lowercase`, `domain_matching_is_label_aligned`; `ranking.rs`: `unverified_reviews_do_not_change_search_rating`, `many_keys_one_person_count_once`, `phishing_listing_excluded_from_price_range`, `userinfo_url_not_domain_verified`, `lookalike_hosts_not_domain_verified`; `purchase.rs`: `valid_receipt_is_verified_purchase`, `receipt_by_non_seller_not_verified`, `review_citing_someone_elses_receipt_not_verified`, `receipt_for_other_product_not_verified`, `seller_review_with_own_receipt_not_verified`, `unverified_seller_receipt_not_verified`, `sybil_buyers_of_verified_seller_not_verified_purchase`, `tampered_receipt_not_verified`, `receipt_survives_listing_retract`, `receipt_after_seller_revoke_cutoff_not_verified`, `one_counted_review_per_author_and_product`, `unverified_buyer_not_verified_purchase_by_default`; `identity.rs`: `attested_author_is_verified`, `expired_attestation_not_verified`, `retracted_attestation_not_verified`, `self_issued_attestation_not_verified`, `untrusted_attestor_not_verified`, `attestor_trusted_for_other_kind_not_verified`, `not_after_seq_cuts_off_attestor`, `forked_attestor_attestations_after_fork_do_not_count`, `revoke_ignores_later_records`, `revoke_from_unrelated_attestor_ignored`, `revoke_by_cut_off_attestor_ignored`, `stolen_key_fork_does_not_restrict_person_after_revoke`, `forked_seller_restricted_on_other_keys`, `attestation_in_future_ignored`; `time.rs`: `future_ts_ignored`, `views_are_pure_functions_of_policy_now`; `views_adversarial.rs`: `verified_person_outranks_sybil_product_facts`, `one_person_many_keys_count_once_in_facts`, `sybil_images_marked_unverified`, `forked_reviewer_excluded_from_ratings`, `seller_alt_key_review_excluded_from_ratings`, `records_after_retire_ignored`, `retire_successor_inherits_nothing`, `mean_rounds_half_to_even`, `views_are_independent_of_ingest_order`, `policy_hash_ignores_order_and_nulls`, `invalid_policy_rejected`; `sparse.rs` (light clients, T18a): `sparse_view_matches_with_forked_reviewer`, `sparse_view_matches_with_restricted_seller_on_other_product`, `sparse_view_matches_with_revoked_and_alt_key_cases` |
| Moderation is visible | Display-only hiding by published threshold; per-person counting and exclusions; records never deleted (7.6) | `moderation.rs`: `verified_flags_hide_at_threshold`, `seller_self_flag_not_counted`, `seller_other_key_same_person_flag_not_counted`, `unverified_sybil_flags_not_counted`, `one_person_many_keys_flags_count_once`, `flag_retraction_unhides`, `appeal_alone_does_not_unhide`, `appeal_by_non_author_ignored`, `product_seller_flags_on_review_not_counted`, `product_seller_alt_key_flags_on_review_not_counted`, `review_hidden_at_its_threshold_still_counts_in_ratings`, `flags_on_flags_hide_nothing`, `restriction_requires_distinct_flaggers`, `restriction_not_diluted_by_dummy_listings`, `retracting_hidden_listings_does_not_escape_restriction`, `hidden_listing_excluded_from_price_range`, `restriction_follows_person_to_new_key`, `hidden_listing_still_anchors_receipts`, `flags_require_verified_off_counts_keys`; `views_adversarial.rs`: `forked_flagger_flags_not_counted`, `future_flags_do_not_count`; `moderation_numbers.rs`: `false_hide_table_matches_doc`, `jury_capture_table_matches_doc` |
| Legal removal is visible | Header kept; control kinds keep bodies (5.3) | `redaction.rs`: `header_only_record_accepted`, `body_restored_when_hash_matches`, `body_with_wrong_hash_rejected`, `control_kind_body_cannot_be_removed`, `removed_body_refused_on_resend`, `removal_list_applies_before_first_receipt`, `control_kind_without_body_has_no_effect` |

Section 9 of this document lists which tests pass against the current crates.

## 7. Residual risks and product gaps

These remain after v1 mitigations. Each lists the exposure and the proposed path.

### R1. Attestor key theft at seq 1 (V16)

A thief with an attestor's key can sign a conflicting record at `seq = 1`. The fork seq is then 1, and views ignore every record by that attestor (5.2). Every identity it attested loses verification at once.

- Exposure: all users of that attestor lose verified status until a successor key re-issues attestations. Flags and verified-purchase reviews by those users stop counting during the outage.
- Mitigations: attestors keep keys in a hardware security module (HSM); each attestor publishes a successor key in advance; the default policy lists at least 2 independent attestors so one outage does not remove all verification.
- Planned: re-issuance tooling that copies all current attestations to a successor key in one batch.

### R2. Backdating with fresh keys (V17)

The `ts` rule binds only within one log. A new key can claim any past `ts` for its first record. A seller can create a fresh key and publish a backdated price history ("was €99").

- Exposure: fake reference prices. EU law requires a price-reduction claim to cite the lowest price of the prior 30 days ([Directive 2019/2161, Art. 2](https://eur-lex.europa.eu/eli/dir/2019/2161/oj)), so fake history can mislead consumers in exactly the way that law targets.
- Mitigation in v1: price history shows the record author and whether the seller is verified and domain-verified.
- Planned: **witness timestamps**. Nodes publish signed checkpoints of the heads they hold, with their own time, similar to [Certificate Transparency](https://www.rfc-editor.org/rfc/rfc9162) logs and [RFC 3161](https://www.rfc-editor.org/rfc/rfc3161) time-stamping. A view uses the median "first seen" time across witnesses that the policy trusts, instead of `ts`, for price history.

### R3. One count per attestor, not per human (V22, part 1)

A person is a connected component of keys linked by shared `(attestor, person)` pairs (7.3). If one key carries attestations from 2 attestors, both pairs fall into one person. A human who gets key K1 verified by attestor X and a different key K2 verified by attestor Y has no shared pair, so the human counts as 2 persons.

- Exposure: with A trusted identity attestors, one human counts up to A times by using one key per attestor. With the default of 2 attestors, a brigade needs half as many humans.
- Mitigations: keep the default attestor list short; attestors may publish a shared, salted pseudonym if they agree on a matching rule (for example a national ID number), but that increases linkage.

### R4. Linkage of a human's keys (V22, part 2)

The `person` value is public and the same for every key of one human at one attestor. Anyone can group all of that human's keys.

- Exposure: a buyer who uses separate keys for separate product areas gains no separation from the public.
- Planned: anonymous credentials (identity.md section 7) replace the public pseudonym with a per-context proof.

### R5. Marketplace sellers cannot domain-verify (V23)

A seller on a marketplace such as amazon.com or etsy.com cannot prove control of that domain. With `price_requires_domain_verified` set, their listings never feed price ranges.

Options:

| Option | How it works | Phishing resistance | Cost |
|---|---|---|---|
| A. Marketplace host list plus corroboration | The policy lists marketplace hosts (`marketplace_hosts`). A listing on such a host feeds prices only if an observation of the same URL by a verified author other than the seller, within 7 days, reports the same price. | URL host is a known marketplace, so the link cannot lead to a phishing domain. | Policy field plus one view rule; needs crawlers or users who observe. |
| B. Storefront attestation | A new kind `attest.storefront { subject, host, store_id, expires }`. The attestor checks that the seller's public storefront page on the marketplace shows a code naming `subject`. Views match listing URLs against the storefront's URL pattern for that host. | Strong for hosts with stable store URL patterns. | One URL pattern per marketplace; attestor work per seller. |

Recommendation: option A first, because it reuses `observation` records and needs no new kind. Option B later for large sellers.

### R6. Extension lookup prefix reveals the product in small catalogs

A 4-hex prefix splits keys into 65,536 buckets. With a catalog of C products, a bucket holds C / 65,536 products on average.

| Catalog size C | Products per bucket |
|---|---|
| 10,000 | 0.15 |
| 100,000 | 1.5 |
| 1,000,000 | 15.3 |
| 10,000,000 | 152.6 |

Below about 1,000,000 products, a lookup that hits a catalog product usually identifies it.

- Proposal: the node reports catalog size in `/v1/status`. The extension picks the longest prefix that leaves at least 20 products per bucket. That is 3 hex characters at C = 100,000 (24 per bucket).

### R7. Stolen key forks restrict the victim (T24)

Under 7.3, a person with any forked key is restricted on all keys. A thief can sign a conflicting record with a stolen key and so restrict the victim.

- Resolved in contract 7.3: a fork on a key that has a counting `attest.revoke` does not count toward per-person status. Truncation of that key's log still applies.
- Remaining exposure: the victim is restricted from the moment of the fork until the attestor publishes the revoke.
- Test: `identity.rs`: `stolen_key_fork_does_not_restrict_person_after_revoke`.

### R8. Seller withholding receipts

Sellers decide whom to give receipts. A seller can give receipts only to buyers who are unlikely to complain, for example repeat customers.

- Exposure: verified-purchase ratings skew positive for that seller.
- Mitigations: issue receipts at checkout, before the outcome is known (verified-purchase.md); show the count of reviews without receipts next to verified-purchase counts.
- Planned: third-party proofs (DKIM, TLS) that need no seller cooperation.

### R9. Brigades of real, verified people

No identity scheme stops a group of real people who agree to act together. The v1 threshold rule lets T verified people hide a listing. moderation.md quantifies this and proposes juries and counter-signals.

### R10. Eclipse when a client knows only one node

A first-time visitor who loads the app from one node sees only that node's data until it learns other nodes. Head comparison needs at least 2 nodes.

- Mitigation: release builds bundle a list of independent bootstrap nodes.

### R11. Sparse stores trust nodes for completeness

A sparse store (5.5) checks each record but cannot tell whether records are missing. A node can omit a newer listing version (showing an old, lower price), a retract, a revoke, flags, or fork evidence.

- Exposure: extension results can be stale or incomplete in the direction the node chooses.
- Mitigation in v1: at least 2 nodes, with disagreement shown (10).
- Planned: each node signs a checkpoint of its heads. The related set includes, per author, the head `(seq, id)` from that checkpoint. A client that receives checkpoints from several operators can then see that it lacks a record, even if it cannot fetch it.

### R12. Keys in IndexedDB are exposed to code in the app's origin

Since contract D15 (amended in v3), the app keeps keys in IndexedDB by default. Any script that runs in the app's origin can read them.

| Path | Who can read the key |
|---|---|
| Cross-site scripting through record text | Any author, if a rendering bug lets text become markup. Contract section 10 forbids markup rendering. |
| Malicious or compromised JavaScript dependency | Its publisher. The web app uses no third-party scripts. |
| Altered app served by a node | That node's operator, for keys created or imported on that node's origin only. |
| Malware on the device | Anyone with user-level access. Out of scope. |

- Guidance for users: create and use keys only in the signed static release or the official build. Do not import a key into an app served by an unknown node.
- Planned: passphrase encryption. It protects copies of the browser profile, not keys in a live session, because altered code can capture the passphrase.

### R13. Sybil storage flood (T15a)

The admission limits in `sync-protocol.md` bound one key: A1 = 1,000,000 records per log and A2 = 10,000 records per author per 24 hours of claimed `ts`. Keys are free, so they do not bound an attacker.

| Attacker keys | Records per day (A2 each) | Bytes per day at 66 KB per record (64 KiB body + header) |
|---|---|---|
| 1 | 10,000 | 0.66 GB |
| 100 | 1,000,000 | 66 GB |
| 10,000 | 100,000,000 | 6.6 TB |

Claimed `ts` makes A2 weaker. A new key can start its log at `ts = 0` (1970) and claim 10,000 records per past day. It can then send up to A1 = 1,000,000 records at once, about 66 GB at the body limit. Counting by arrival time would close this gap, but arrival time differs per node and would break convergence. The tiers below bound it deterministically instead: an unverified key gets at most the tier-3 cap, and a verified person is accountable through restriction and the attestor.

Proposed mitigation: **admission tiers**. Each node sorts authors into tiers using its own **admission attestor set**. The default admission set is the union of all attestors in the default policy.

| Tier | Authors | Replication | Limits |
|---|---|---|---|
| 1 | Admission attestors, and authors with a counting `attest.identity` or `attest.domain` from one of them | Always, in full | A1, A2 per **person key**, not per key, so one person with many keys shares one budget |
| 2 | Unverified authors whose records are referenced by tier-1 records: the target author of a tier-1 `flag` or `appeal`, or the seller of a listing cited in a tier-1 review's receipt | Always, up to a per-author cap (proposed: 1,000 records or 16 MiB) | The cap |
| 3 | All other authors | Best effort, within a global budget B (proposed default: 1 GiB) and a per-author cap (proposed: 100 records or 1 MiB) | B and the cap |

Rules that keep honest nodes convergent:

1. Tier membership depends only on records and the admission set, so two honest nodes with the same records and the same admission set compute the same tiers.
2. Tiers 1 and 2 are never evicted while their limits hold. Therefore honest nodes converge on every verified log and every log that verified authors point to.
3. In tier 3, a node keeps authors in ascending order of `sha256(author || epoch)` until B is reached. `epoch` is the current UTC day divided by 30, so the order is the same on all nodes and changes monthly. An attacker cannot choose a low rank without grinding keys. Nodes with the same candidate set and the same B keep the same tier-3 authors. Nodes with different candidate sets may differ. That is acceptable, because tier-3 records do not feed ranking (7.8) or counted flags (7.6).
4. Eviction removes a whole log, or truncates it to a shorter prefix. Both keep the complete-prefix rule (5.1).
5. An author moves to tier 1 as soon as a counting attestation arrives. The node then fetches the full log from peers.

Effect on views: verified-only results (ranking, counted flags, verified-purchase ratings) are identical on all honest nodes that share an admission set covering the user's `trusted_attestors`. The secondary all-reviews `RatingSummary` can differ between nodes. The app labels it as best effort.

Weaker alternatives considered:

| Alternative | Problem |
|---|---|
| Proof of work on a key's first record | [Hashcash](http://www.hashcash.org/papers/hashcash.pdf)-style costs are small for attackers with GPUs and large for honest phones. Needs a body field and a contract change. |
| First come, first kept for unverified authors | Arrival order differs per node, so tier-3 contents would never converge, and an attacker who floods first wins. |
| Reject all unverified authors | Blocks new users from posting their first records before they get verified. |

Adopted into contract section 9. Original request: add the three tiers as deterministic admission rules, and apply A1 and A2 per person key in tier 1. Tier membership needs view logic (counting attestations), so `perch-views` should export the tier function for `perch-node` to call.

### R14. Third-party verifiers accept a universal forgery (T4)

Contract D2 lets anyone verify records with standard tools such as Web Crypto. Two common verifiers accept a forgery that perch-core rejects (data-modeler measurement, 2026-10-18):

| Verifier | Rejects `S ≥ L` | Rejects small-order or non-canonical `A`, `R` | Accepts the forgery below |
|---|---|---|---|
| Web Crypto Ed25519 (Node 22.22) | Yes | No | Yes |
| Python `cryptography` 50.0.1 | Yes | No | Yes |
| perch-core (`verify_strict` rules, 4.2) | Yes | Yes | No |

The forgery: author `0100000000000000000000000000000000000000000000000000000000000000` (the identity point) and signature `0100…00` ‖ `00…00` (R = identity, S = 0). The equation `[S]B = R + [k]A` holds for every message, because every term is the identity.

- Exposure: a third-party tool that verifies with Web Crypto or Python alone accepts any record "signed" by that author, including attestations and receipts. Perch clients and nodes are not affected.
- Mitigation: the contract's rule (4.2) requires rejecting small-order `A` and `R`. Third-party verifiers must add that check before calling the library: reject if `A` or `R` is one of the 8 small-order encodings (`crates/perch-tests/src/vectors.rs` lists 5 of them; non-canonical encodings add the rest), or multiply by the cofactor and compare with the identity.
- Tests: `isolated.rs`: `universal_forgery_identity_key_rejected`, `small_order_public_key_rejected`.
- Published: [`docs/test-vectors.json`](test-vectors.json) holds 7 valid records, 1 receipt, and 13 invalid inputs, including this forgery as `identity_point_forgery`. Node 22 Web Crypto accepts that vector for any message (checked 2026-10-18); perch-core rejects it. Planned: add S + L malleation and each small-order encoding.

## 8. Legal exposure

This is an engineering summary, not legal advice.

| Issue | Rule | Perch design response |
|---|---|---|
| Illegal content in an append-only log | EU [Digital Services Act](https://eur-lex.europa.eu/eli/reg/2022/2065/oj) Art. 6: a host keeps its liability exemption if it acts quickly to remove illegal content once it knows of it. Art. 16 requires a notice mechanism. | Body removal keeps logs verifiable (5.3). Each node keeps its own removal list. |
| Personal data and erasure | GDPR Art. 17. The [EDPB Guidelines 02/2025 on blockchain](https://www.edpb.europa.eu/system/files/2026-07/edpb_guidelines_202502_blockchain_v2_en.pdf) advise against storing personal data on-chain in the clear. Where deletion is impossible, they advise making data unidentifiable, for example by deleting off-chain parts. | Headers hold keys, hashes, integers, and `kind` only. Free text sits in removable bodies. Receipts stay off the log unless the buyer cites one. The `person` pseudonym is personal data under GDPR because the attestor can re-identify it. |
| Defamation in reviews | US: [47 U.S.C. § 230](https://www.law.cornell.edu/uscode/text/47/230) protects hosts of third-party content. Other jurisdictions use notice-based rules. | Node operators can remove bodies on notice. Authors of reviews are known keys and often attested people. |
| Fake reviews and review suppression | US FTC rule, [16 CFR Part 465](https://www.govinfo.gov/content/pkg/FR-2024-08-22/pdf/2024-18519.pdf), effective 2024-10-21: bans buying fake reviews and suppressing reviews; penalties up to $51,744 per violation. | Perch records who said what. Sellers who issue receipts to sybils or withhold them can be identified from public data and from their own attestation. |
| Price claims | EU Directive 2019/2161 (prior-price rule) | Price history shows its source; witness timestamps planned (R2). |

## 9. Test status

The adversarial tests live in `crates/perch-tests`. Run them with:

```sh
cargo test -p perch-tests
```

Tests are grouped by the layer they need. A test whose layer is not yet wired into `src/sut.rs` prints `SKIP` and passes.

| Layer | Test files | Status (2026-10-18) |
|---|---|---|
| Core (`perch-core`) | `isolated.rs`, `encoding.rs`, `limits.rs`, `core_semantics.rs` | 44 of 44 pass |
| Store (`perch-store`) | `log.rs`, `redaction.rs` | 22 of 22 pass |
| Sync (`perch-sync`) | `sync.rs` | 15 of 15 pass |
| Views (`perch-views`) | `references.rs`, `identity.rs`, `purchase.rs`, `moderation.rs`, `ranking.rs`, `time.rs`, `views_adversarial.rs`, `sparse.rs`, 1 in `redaction.rs`, 1 in `sync.rs` | 81 of 81 pass |
| None (fixtures, doc numbers) | `src/` unit tests, `moderation_numbers.rs` | 9 of 9 pass |
| Total | | 171 of 171 pass; 1 ignored (timing, `verify_cost.rs`) |

### 9.1 Measured verification cost

`tests/verify_cost.rs` measures strict Ed25519 verification (ed25519-dalek 3.0, release build, shared 2-core build machine, 400-byte message, 20,000 runs):

| Operation | Time per signature |
|---|---|
| `verify_strict` | 44.5 µs |

At that rate, one core verifies about 22,000 records per second. Verifying 10,000 records takes about 0.45 s at this rate. Wasm speed is not measured here.

Run it with:

```sh
cargo test -p perch-tests --release --test verify_cost -- --ignored --nocapture
```

## Appendix A: terms and acronyms

| Term | Meaning |
|---|---|
| Brigading | Many people acting together to push a rating or moderation outcome. |
| DKIM | DomainKeys Identified Mail ([RFC 6376](https://www.rfc-editor.org/rfc/rfc6376)): a mail server's signature over a message. |
| DSA | EU Digital Services Act. |
| EDPB | European Data Protection Board. |
| Eclipse attack | Isolating a client so that it sees only data chosen by the attacker. |
| FTC | US Federal Trade Commission. |
| GDPR | EU General Data Protection Regulation. |
| HSM | Hardware security module: a device that holds a key and signs without revealing it. |
| JCS | JSON Canonicalization Scheme ([RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)). |
| Person | A connected component of keys linked by shared `(attestor, person)` pairs; counted once (contract 7.3). |
| Person key | The lowest `(attestor, person)` pair in a person; used to identify and sort persons. |
| Small-order point | An elliptic-curve point whose multiples cycle through a few values; a public key made from one can accept signatures on many messages. |
| Sybil | One actor posing as many. |
| Witness | A party that signs what it has seen and when, so that others can check timing. |
