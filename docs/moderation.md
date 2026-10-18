# Perch moderation

Status: draft for contract version 3 (2026-10-18). Owner: security.

This document analyzes the v1 moderation rule, compares alternatives with numbers, and recommends what v1 ships and what to evaluate later. Section numbers such as "7.6" refer to [`ARCHITECTURE.md`](../ARCHITECTURE.md). The user asked for brainstorming on this hard problem; section 6 collects further ideas.

## 1. Constraints

Any Perch moderation rule must satisfy these, because of the contract:

| # | Constraint | Source |
|---|---|---|
| C1 | Deterministic: the same records and policy give the same result on every device. No clock, no randomness except from records. | 7 |
| C2 | Inputs are public records and the policy only. No hidden reports, no moderator dashboards. | 1, D5 |
| C3 | Hiding never deletes. It changes display only, for listings, reviews, products, and profiles. | 7.6, D16 |
| C4 | Counts are per verified person, never per key. | 7.3, D13 |
| C5 | Users can change the policy. | 7.1 |

C1 rules out methods that retrain models from random starting points. C2 rules out private reports.

## 2. The v1 rule

From 7.6:

- **Hide:** a listing, review, product, or profile is hidden when at least `flag_threshold` (default 3; per kind via `flag_thresholds`) distinct verified persons have unretracted flags on its root. The target's author never counts.
- **Appeal:** the target's author can publish an appeal. It has no effect by itself. Status changes only when flaggers retract.
- **Restrict:** a seller person is restricted when at least `seller_hidden_listing_limit` (default 3) of their listings are hidden, and at least `seller_min_distinct_flaggers` (default 9) distinct persons flagged those hidden listings. Restriction follows the person to every key.

This is the user's proposal: enough users flagging fraud hides a post, appeals exist, and repeat fraud restricts the seller.

## 3. Failure modes of the v1 rule, with numbers

### 3.1 False hides on popular listings

Assume each verified person who views a listing flags it by mistake, or out of spite, with probability q. If E verified persons view a listing, the number of false flags follows a binomial distribution. The probability that a listing with no real problem is hidden is P(flags ≥ T):

| q | E = 100 | E = 1,000 | E = 10,000 |
|---|---|---|---|
| T = 3, q = 0.0005 | 0.00% | 1.44% | 87.5% |
| T = 3, q = 0.001 | 0.02% | 8.02% | 99.7% |
| T = 5, q = 0.001 | 0.00% | 0.36% | 97.1% |
| T = 10, q = 0.001 | 0.00% | 0.00% | 54.2% |
| T = 20, q = 0.001 | 0.00% | 0.00% | 0.34% |

A fixed threshold that suits a small listing hides almost every popular one. q is unknown for Perch, so the values above illustrate the effect and do not predict rates. Section 9 gives the method.

### 3.2 Brigading by real people

The v1 rule assumes flaggers act independently. A group acting together needs:

| Goal | Persons needed at defaults |
|---|---|
| Hide one listing | T = 3 |
| Hide any number of listings | 3 (the same 3 people, no limit) |
| Restrict a seller | max(F, T) = 9 distinct persons, flagging at least L = 3 listings |

Recruitment cost: one study of paid review groups found that sellers refund the product and, for about 15% of products, pay an average commission of $6.24 ([He, Hollenbeck, Proserpio 2022](https://cacm.acm.org/magazines/2023/10/276636-leveraging-social-media-to-buy-fake-reviews/fulltext)). A flag needs no purchase. If a flag farm charged the same commission, hiding a listing would cost about 3 × $6.24 = $18.72, and restricting a seller about 9 × $6.24 = $56.16. These are estimates from a review market, not measured flag prices.

### 3.3 Hidden negative reviews

A seller can hire 3 verified people to flag every negative review of its product. In v3 hiding is display-only (7.6), so hidden reviews still count in ratings. The review text is still collapsed, which is what readers look at.

### 3.4 No way back

Appeals change nothing unless flaggers retract. Flaggers have no reason to return. A false hide is therefore permanent in practice.

### 3.5 Victims cannot always flag

`flags_require_verified` means a defrauded buyer without a verified identity cannot help hide a fraudulent listing. Verification costs time and about $1.50 per ID check at commercial services ([Stripe Identity](https://stripe.com/identity)). Early in the network, few buyers are verified, so fraud may stay visible longer.

### 3.6 Backdating

A flag's `ts` is claimed by its author. Rules that use time (decay, rate limits) can be gamed by a fresh key that backdates. Any time rule should use `max(flag ts, ts of the flagger's counting attestation)`: a person cannot have flagged before they were verified.

## 4. Alternatives

### 4.1 Audience-relative threshold

Hide when flags ≥ max(T, ⌈r × E⌉), where E is a measure of the target's audience that records can show. Perch has no view counts, so E must come from records, for example the number of distinct verified persons who reviewed the product.

With q = 0.001:

| E | r = 0 (v1) | r = 0.5% | r = 1% |
|---|---|---|---|
| 100 | need 3; false hide 0.02% | need 3; 0.02% | need 3; 0.02% |
| 1,000 | need 3; 8.02% | need 5; 0.36% | need 10; 0.00% |
| 10,000 | need 3; 99.7% | need 50; 0.00% | need 100; 0.00% |

- Strength: fixes 3.1. Deterministic.
- Failure: a seller can inflate E with friends' reviews to raise the bar. Each extra reviewer must be a verified person, so raising the bar by 1 flag at r = 1% costs 100 verified reviewers.
- Cost: one policy field and one count in views.

### 4.2 Reputation-weighted flags

Weight each flagger by past accuracy, for example the share of their past flags on targets that ended up hidden.

- Failure 1, no ground truth: "accuracy" is defined by the same flags. Early majorities become self-confirming.
- Failure 2, reputation farming: a farm builds weight on obvious spam, then spends it on one target.
- Failure 3, cold start: new honest users have no weight, so fraud victims, who flag once, count least.
- Verdict: do not use as the main rule. Usable as a tie-breaker once juries (4.4) provide independent outcomes.

### 4.3 Deposits

Flaggers lock a deposit and lose it if the flag is overturned.

- Strength: makes mass flagging expensive.
- Failure: Perch has no payment layer, and adding one adds a custodian. Deposits deter low-income honest users more than funded attackers. A neutral party must decide "overturned".
- Verdict: out of scope for v1 and v2.

### 4.4 Juries of randomly selected verified persons

When the target's author appeals, a jury of n verified persons is drawn from a public random value. A majority decides. Jurors publish signed votes as records.

Probability that colluders hold a majority, if a fraction p of eligible persons collude:

| Jury size n | p = 5% | p = 10% | p = 20% | p = 30% |
|---|---|---|---|---|
| 5 | 0.116% | 0.856% | 5.79% | 16.3% |
| 9 | 0.003% | 0.089% | 1.96% | 9.88% |
| 15 | 0.000% | 0.003% | 0.42% | 5.00% |
| 21 | 0.000% | 0.000% | 0.10% | 2.64% |

Requirements and failure modes:

- **Randomness must not be chosen by a party to the case.** If the seed were the appeal's record ID, the appellant could try new appeals until a friendly jury appears. At n = 5 and p = 10%, that takes about 117 tries on average; at n = 9, about 1,122. Use a public randomness beacon round that occurs after the appeal is published, such as [drand](https://drand.love), run by 17 organizations in the League of Entropy. A trusted author relays the beacon value as a record. Views verify the beacon's signature, which needs pairing code in wasm.
- **Eligible pool must be fixed before the draw**, for example persons verified before the appeal's beacon round, sorted by person key.
- **Jurors may not answer.** Draw extra jurors in order and take the first n votes within a deadline in beacon rounds. If fewer than n vote, the status does not change.
- **Bribery after selection.** Jurors' identities are public once drawn. Votes are public records. A briber can pay and check. Commit-reveal voting hides votes until the deadline but does not stop a briber who asks for the opening.
- **Juror workload.** Each case costs n people several minutes. [Kleros](https://ethresear.ch/t/cryptoeconomics-in-kleros/1401) discussions estimate about $20 of juror time per case at 5 jurors.
- Kleros pays jurors and penalizes votes against the majority. Perch has no payments, so juror motivation must be civic or reputational.
- Verdict: the best candidate for appeals. Evaluate in v2. It also matches the user's idea of appeals decided by users: the pool can be restricted to verified persons with receipts from the same listing (section 4.9).

### 4.5 Bridging (Community Notes style)

X's Community Notes shows a note only if raters who usually disagree both rate it helpful. A matrix factorization model predicts each rating as `μ + i_u + i_n + f_u · f_n`. A note is shown if its intercept `i_n ≥ 0.40` and `|f_n| < 0.50`; the intercept regularization (0.15) is 5 times the factor regularization (0.03) ([Community Notes ranking documentation](https://github.com/twitter/communitynotes/blob/main/documentation/under-the-hood/ranking-notes.md)). Fewer than 10% of notes reach "Helpful". Posts with a note received 25–34% fewer likes or retweets ([Wojcik et al. 2022](https://arxiv.org/abs/2210.15723)).

For Perch, a flag would count only when flaggers come from different factor groups.

- Strength: a brigade from one group cannot act alone.
- Failure 1, determinism (C1): the production model retrains from scratch every hour and re-fits if the loss exceeds 0.09 after an unlucky start. Perch would need a fixed seed, a fixed iteration count, and fixed-point or strictly specified floating-point arithmetic so that wasm and native agree.
- Failure 2, data density: factors need many ratings per person. Perch flags are sparse; most persons flag rarely.
- Failure 3, fraud is not polarized: a scam listing is not a political topic. Bridging helps where opinion splits, such as review disputes, more than for fraud.
- Verdict: evaluate for review flags only, after a year of data.

### 4.6 Per-category and per-reason thresholds

`flag_thresholds` already sets thresholds per target kind. Extend it per reason or per product category.

| Example | Rationale |
|---|---|
| `counterfeit` on `health/*` products: threshold 2 | Higher harm |
| `spam` on reviews: threshold 5 | Lower harm; frequent false flags |
| `illegal`, `personal_data`: no hiding by count | These go to node operators' legal removal (5.3) and need a human decision |

- Failure: an attacker picks the reason with the lowest threshold. Views must count a flag toward one reason's threshold only, and the lowest threshold must still be at least 2 persons.

### 4.7 Time decay

A flag counts for D days after `max(flag ts, attestation ts)` (see 3.6). Example: D = 180.

- Strength: fixes 3.4. Stale flags expire.
- Failure: a brigade re-flags. Each person can count once per target. A flag can be replaced (4.5 of the contract), so decay must use the chain root's `ts`, or a replacement would renew it.
- Also needed: hidden fraud listings would reappear after D days. Pair decay with restriction, which persists while hidden counts persist.

### 4.8 Rate limits

Count at most K flags per person per window W, in `(ts, id)` order.

With 9 colluders, T = 3, K = 10, and W = 30 days, the brigade can hide at most 9 × 10 / 3 = 30 listings per 30 days. Without a limit, it can hide any number.

- Failure: honest heavy users (consumer groups, crawlers checking fraud) reach the limit. Policies can exempt listed moderators (4.9).

### 4.9 Counter-signals from buyers of the same listing

The user proposed that appeals involve users of the same listing. One rule: a hidden listing becomes visible when V distinct verified persons with valid receipts from that listing publish a `vouch` for it.

- Strength: only real buyers can vouch, and fraud victims will not.
- Failure: the seller issues receipts to friends. Each must be a verified, distinct person, so the cost to the seller is V recruited people, the same order as a brigade.
- Safer form: vouches do not unhide by themselves. They trigger a jury (4.4) drawn from the listing's verified buyers, or from all verified persons if buyers are too few.

### 4.10 User-selectable moderators ("labelers")

Bluesky lets anyone run a labeling service, and users subscribe to the ones they choose ([Bluesky moderation architecture](https://docs.bsky.app/blog/blueskys-moderation-architecture)). In Perch, a policy field `trusted_moderators: [author]` would make a single flag from a listed moderator enough to hide.

- Strength: deterministic, transparent, cheap. Professional moderators (consumer groups) can act quickly on fraud. Users choose whom to trust, and see who hid what.
- Failure: users concentrate on one or two popular moderators, which recentralizes moderation. A captured moderator can hide anything, but users can see this and switch.
- Verdict: cheap and compatible with the contract model. Recommend for v1.1.

### 4.11 Summary

| Method | Fixes | Main failure | Deterministic | Cost |
|---|---|---|---|---|
| v1 threshold | Sybils (with C4) | False hides at scale; brigades of 3 | Yes | Shipped |
| Audience-relative threshold | False hides | Audience inflation by friends | Yes | Low |
| Reputation weight | — | No ground truth; farming | Yes | Medium |
| Deposits | Mass flagging | Needs payments and adjudicator | — | High |
| Juries | Brigades; appeals | Bribery; workload; beacon needed | Yes, with beacon records | High |
| Bridging | Polarized brigades | Determinism; sparse data | Hard | High |
| Per-reason thresholds | Harm-sensitivity | Reason shopping | Yes | Low |
| Time decay | Permanent false hides | Re-flagging | Yes | Low |
| Rate limits | Brigades on many targets | Heavy honest users | Yes | Low |
| Buyer vouches | Appeals | Seller-recruited vouchers | Yes | Medium |
| Moderator lists | Slow fraud response | Recentralization | Yes | Low |

## 5. Recommendation

### 5.1 v1 (ships now; no contract change)

1. The rule in 7.6, with these defaults:

   | Field | Default | Reason |
   |---|---|---|
   | `flag_threshold` | 3 | Early listings have small audiences: P(false hide) ≈ 0.02% at E = 100, q = 0.001 |
   | `flag_thresholds.review` | 5 | Hiding review text is the cheapest way for a seller to bury criticism (3.3) |
   | `flag_thresholds.product` | 5 | Product records are shared by many sellers |
   | `flags_require_verified` | true | Free keys cost nothing (C4) |
   | `seller_hidden_listing_limit` | 3 | User's "repeat fraud" rule |
   | `seller_min_distinct_flaggers` | 9 | A restricting brigade needs at least 9 persons |

2. **Flags on reviews by any seller of the product do not count** (adopted into 7.6). A seller cannot hire flaggers on its own keys to hide reviews of its product. Hired verified persons still count; see section 3.3.
3. The app shows, for every hidden item: the flag count, the reasons, each flagger's key and verification, and the appeal text. Users can expand hidden items.
4. The app shows the seller's appeal next to the flags.

These defaults are in the contract's default policy (7.1).

### 5.2 v1.1 (small contract changes)

1. **Time floor for flags:** a flag's effective time is `max(flag root ts, flagger's counting attestation ts)` (3.6).
2. **Time decay:** flags count for 180 days from the effective time (4.7).
3. **Rate limit:** at most 10 counted flags per person per 30 days (4.8).
4. **Moderator lists:** `trusted_moderators` in the policy (4.10).
5. **Audience-relative threshold:** `flag_threshold_reviewer_permille`, with E = distinct verified persons who reviewed the product (4.1). Start at 5‰ (0.5%).

### 5.3 v2 (evaluate)

1. Juries for appeals, drawn with a relayed beacon, pool first from verified buyers of the listing (4.4, 4.9).
2. Per-reason thresholds, with `illegal` and `personal_data` routed to legal removal (4.6).
3. Bridging for review flags once data exists (4.5).
4. Anonymous flags with one pseudonym per target (identity.md section 7).

## 6. Brainstorm: further ideas

These are not analyzed in depth.

- **Warn before hide.** Two thresholds: at T₁ the item shows a warning; at T₂ it is hidden. Readers get information without full hiding. Needs a new status value.
- **Evidence-weighted flags.** A `counterfeit` or `fraud` flag from a verified buyer who holds a receipt from that listing counts double. A fraudulent seller may refuse receipts, so this can only add weight, never be required, until DKIM proofs exist (verified-purchase.md).
- **Seller response.** Show the seller's appeal text inline with the flags, so readers judge both sides.
- **Flagger accountability.** If a jury overturns a hide, flags by those flaggers on the same seller stop counting for 90 days.
- **Restriction decay.** Restriction ends when fewer than L hidden listings remain within the decay window, so a seller who fixed problems can recover.
- **Local-only hiding.** Each user's own flags hide items for that user immediately, with no threshold. This gives a quick personal response without affecting others.
- **Cross-attestor agreement.** Count a flag toward restriction only if flaggers come from at least 2 different identity attestors, so one corrupt attestor's users cannot restrict a seller alone.

## 7. Tests

Tests in `crates/perch-tests/tests/moderation.rs` check the v1 rule:

| Test | Checks |
|---|---|
| `verified_flags_hide_at_threshold` | T distinct verified persons hide; T − 1 do not |
| `seller_self_flag_not_counted` | Target's author never counts |
| `product_seller_flags_on_review_not_counted` | Flags by a seller of the product on its reviews do not count |
| `unverified_sybil_flags_not_counted` | Unverified authors do not count when `flags_require_verified` |
| `one_person_many_keys_flags_count_once` | Person key counting |
| `flag_retraction_unhides` | Retracted flags stop counting |
| `restriction_requires_distinct_flaggers` | L hidden listings with fewer than F distinct flaggers do not restrict |
| `restriction_follows_person_to_new_key` | Restriction applies to the seller's other keys |
| `hidden_listing_still_anchors_receipts` | Hiding is display-only |

## 8. Open questions

1. What is q, the per-viewer false-flag rate? Measure it from the first months of data.
2. Who relays beacon values, and how do views verify BLS signatures in wasm?
3. Should restriction ever end without the seller's flags being retracted?

## 9. Method for the numbers

- Section 3.1 and 4.1: P(X ≥ T) for X ~ Binomial(E, q).
- Section 4.4: P(X ≥ ⌊n/2⌋ + 1) for X ~ Binomial(n, p). Grinding tries: 1 / that probability.
- Section 4.8: maximum hidden listings = persons × K / T.

## Appendix A: terms and acronyms

| Term | Meaning |
|---|---|
| Beacon | A public service that publishes unpredictable random values at fixed times, signed so that anyone can check them. |
| BLS | Boneh–Lynn–Shacham signatures, used by drand. |
| Brigade | A group acting together to push a moderation outcome. |
| Bridging | Ranking that favors content approved by people who usually disagree. |
| Commit-reveal | Voting in two steps: publish a hash of the vote, then the vote. |
| Grinding | Retrying a choice until a random outcome favors the attacker. |
| Labeler | In Bluesky, a service that publishes moderation labels users can subscribe to. |
| Matrix factorization | A model that explains ratings as products of hidden per-user and per-item factors. |
| Person | A connected component of keys linked by shared `(attestor, person)` pairs; counted once (contract 7.3). |
| Person key | The lowest `(attestor, person)` pair in a person; used to identify and sort persons. |
