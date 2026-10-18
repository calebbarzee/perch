# Perch UX specification

Status: draft for contract version 3 (2026-10-18). Owner: ux. Readers: app-engineer (web app, extension, browser tests), reviewers.

This document specifies what the official web app and browser extension show, in what order, with what words, and how they look. Section numbers such as "7.6" refer to [`ARCHITECTURE.md`](../ARCHITECTURE.md). "views §9.3" refers to [`docs/views.md`](views.md). When this document conflicts with `ARCHITECTURE.md`, the contract wins.

## Contents

1. Scope
2. Research basis
3. Personas and jobs
4. Trust UI principles
5. Information architecture and navigation
6. Shared components
7. Global states
8. Screen specs
9. Browser extension
10. Visual direction and tokens
11. Accessibility requirements
12. Copy guidelines
13. User-facing glossary
14. Contract dependencies and open questions
15. Changes for the app
- Appendix A: terms and acronyms

## 1. Scope

- In scope: the web app in `web/`, the extension in `extension/`, and the words both use.
- Out of scope: node operator tools, attestor tools, and seller checkout plugins. They are mentioned only where the app links to them.
- Platforms: current Chromium, Firefox, and Safari; 360 px to 1,920 px wide; mouse, touch, keyboard, and screen readers.

## 2. Research basis

Each finding below shapes at least one rule in this document. The rule IDs refer to section 4.

| Finding | Source | Rule |
|---|---|---|
| 95% of test users relied on reviews to learn about products. The rating distribution was the most-used part of the reviews section. 53% actively sought negative reviews. | [Baymard Institute, ratings distribution summary](https://baymard.com/blog/user-ratings-distribution-summary) | T6, Product screen |
| When star filtering was hidden by default, 7% of test users filtered by rating. 90% of those who wanted reviews at a specific rating clicked the distribution bars. | Same | Product screen |
| Nearly twice as many respondents chose a 4.5-star product rated by 57 users over a 5-star product rated by 4 users (more than 5,170 respondents across five surveys). | [Baymard Institute, user perception of product ratings](https://baymard.com/research-articles/user-perception-of-product-ratings) | T6 |
| 132 participants chose the product with more reviews even when both products had the same low rating. In Amazon data, review count and average rating were unrelated. | [Powell et al. 2017, Psychological Science](https://www.psychologicalscience.org/news/releases/people-favor-highly-reviewed-products-even-when-they-shouldnt.html) | T6 |
| Four factors signal credibility: design quality, up-front disclosure, comprehensive and current content, and connection to the rest of the web. | [Harley 2016, Nielsen Norman Group](https://www.nngroup.com/articles/trustworthy-design/) | T1, T2 |
| Show the most important options first and the rest on request. Designs with more than 2 disclosure levels lose users. | [Nielsen 2006, Nielsen Norman Group](https://www.nngroup.com/articles/progressive-disclosure/) | T2, section 6 |
| A crawl of about 53,000 product pages on about 11,000 shopping sites found 1,818 dark-pattern instances of 15 types. 183 sites used deceptive practices. | [Mathur et al. 2019](https://arxiv.org/abs/1907.07032v2) | T7 |
| US law forbids presenting displayed reviews as all or most reviews while suppressing reviews by rating or sentiment. Removal by neutral criteria, applied to all reviews, is allowed. | [FTC rule 16 CFR 465.7](https://www.ecfr.gov/current/title-16/chapter-I/subchapter-D/part-465), effective 2024-10-21 | T3 |
| Ranking systems must explain their main parameters and let users change them. | [EU Digital Services Act, Article 27](https://eur-lex.europa.eu/eli/reg/2022/2065/oj) | T7 |
| Error messages say what happened and how to fix it, reuse the field's label, and avoid "invalid", "please", "sorry", and "oops". | [GOV.UK Design System, error message](https://design-system.service.gov.uk/components/error-message/) | section 12 |
| Pointer targets are at least 24 × 24 CSS px. Focus must not be fully hidden. Copy and paste must work for authentication steps. | [WCAG 2.2, new success criteria](https://www.w3.org/WAI/standards-guidelines/wcag/new-in-22/) | section 11 |
| `Intl.NumberFormat` fraction digits differ from ISO 4217 minor units for 12 of 32 currencies checked in Node 22 (ICU), for example IQD (Intl 0, ISO 3) and LBP (Intl 0, ISO 2). | Measured in this repository's container; [ISO 4217](https://www.iso.org/iso-4217-currency-codes.html) | section 6.9 |
| `format()` given a string uses the exact decimal value; given a Number it can lose precision above 2^53. | [MDN, Intl.NumberFormat.format](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Intl/NumberFormat/format) | section 6.9 |
| `activeTab` gives temporary access to the current tab after a user gesture and shows no install warning. | [Chrome extensions, activeTab](https://developer.chrome.com/docs/extensions/develop/concepts/activeTab) | section 9 |
| Schema.org `Product` carries `gtin`, `gtin8`, `gtin12`, `gtin13`, and `gtin14`. Only `gtin` may also hold a GS1 Digital Link URL. | [schema.org Product](https://schema.org/Product) | section 9 |

Two measurements from this repository's container (Node 22, 1 core):

- Ed25519 key generation (`crypto.generateKeyPairSync`) runs at 21,848 keys per second on 1 core. Key counts therefore mean nothing, which is why ratings lead with identity-verified reviewers (7.8).
- Finding a key whose first 8 hex characters (32 bits) match a target takes about 2^32 / 21,848 s = 54.6 hours on one core. A 15-bit fingerprint glyph takes about 1.5 s. Short key forms are therefore reference aids, never proof of identity (section 6.1).

## 3. Personas and jobs

Each persona lists its jobs, the screens that serve them, and how the Phase 2 review will test them.

### 3.1 Shopper comparing products ("Maya")

Maya wants a good product at a fair price. She has 2 minutes and uses a phone.

| Job | Screens | Test task |
|---|---|---|
| Find a product by name or barcode number. | Search | Find the product with GTIN 4006381333931. |
| See the rating, and whether real buyers gave it. | Product: rating summary | Say the mean rating from verified purchases and how many there are. |
| Read the worst reviews. | Product: reviews, star filter | Show only 1-star and 2-star reviews in at most 2 actions. |
| Compare prices and find a safe shop. | Product: listings; Listing | Name the lowest price from a domain-verified seller. |
| Check a product while on a shop's page. | Extension panel | Read the rating without leaving the shop page. |

### 3.2 Careful researcher ("Tomás")

Tomás buys expensive items and reads everything. He uses a laptop.

| Job | Screens | Test task |
|---|---|---|
| Understand why the rating is what it is. | "How this was computed" disclosures | Name the reviews excluded from the mean and why. |
| See conflicting facts about a product. | Product: facts | Say which sources claim each of 2 conflicting weights. |
| Judge a seller's history. | Author profile | Say whether the seller is restricted and on what numbers. |
| See moderated content and decide for himself. | Moderation notice | Open a hidden review, read the flags, read the appeal. |
| Track price changes. | Price history chart and table | Say the price on a given date and who stated it. |

### 3.3 Small seller ("Aiko")

Aiko runs one web shop and sells 40 products.

| Job | Screens | Test task |
|---|---|---|
| Create an identity and back it up. | Keys | Create a key and download a backup. |
| Get identity and domain verified. | Keys: verification | Copy the exact `perch.json` content for her shop. |
| Publish and update listings. | Publish: listing | Change a price; see that flags stay on the listing. |
| Give buyers receipts. | Listing (own): Issue receipt | Issue a receipt to a buyer's key and copy it. |
| Respond to flags. | Publish: appeal; Author profile | Find which of her listings are hidden and appeal one. |

### 3.4 Independent tester or crawler operator ("Priya")

Priya runs a lab that tests kettles, and a crawler that records shop prices.

| Job | Screens | Test task |
|---|---|---|
| Publish reviews without a purchase receipt, labeled as such. | Publish: review | Publish a review; see "No receipt" on it. |
| Publish price observations in bulk. | Data: import | Import a JSONL file; read the per-record result. |
| Check her records reached the network. | Network; Author profile | Say how many nodes have her latest record. |
| Run a node. | Network | Add her node; see whether it agrees with the others. |

### 3.5 Skeptic auditing the data ("Sam")

Sam does not trust Perch and wants to check it independently.

| Job | Screens | Test task |
|---|---|---|
| Verify any record without Perch code. | Data: record inspector | Copy a record and verify its signature with the shown OpenSSL commands. |
| Check that the running code is the published release. | Footer build info | Compare the wasm SHA-256 with the release page. |
| Check that a node is not hiding data. | Network: head comparison | Find which node lacks records. |
| Reproduce a number. | Policy; Data: export | Export records and policy; read the policy hash. |
| Change the rules and see the effect. | Policy | Set the flag threshold to 10 and see a hidden review become visible. |

## 4. Trust UI principles

These rules apply to every screen. Each rule has an ID that later sections cite.

### T1. Provenance for every fact

- Every value that comes from a record shows its author and a link to the signed record.
- The author is shown as an author chip (section 6.1): display name, verification state, and short key.
- A value with several sources shows the number of sources and the first source. "3 sources" opens the full list.
- A value with conflicts shows "Other values: 2" next to it. Opening it lists each value with its sources (views §10.4).
- Times from records are claims. Copy says "stated" or "claimed": "Price stated on 3 Oct 2026". The record inspector shows the exact `ts`.

### T2. "How this was computed" for every aggregate

Every number computed from several records has a disclosure next to it. It is a `<details>` element with the summary text "How this was computed". It contains, in this order:

1. **Result and inputs.** The number and the count of records used, for example "Mean 4.21 from 37 reviews".
2. **Exclusions.** Each excluded group with its count and reason, for example "Not counted: 2 by sellers of this product; 1 withdrawn by its author".
3. **Policy values used.** Each policy field that affects the number, with its value and a link to Policy, for example "`verified_purchase_requires_verified_buyer`: true".
4. **Rule.** One plain sentence and a link to the views.md section.
5. **Reproduce.** The policy hash (short form, copy button) and the `policy.now` used, as a date and time.

Aggregates that need this disclosure:

| Aggregate | View field | views section |
|---|---|---|
| Rating mean and histogram (each of 3 groups) | `RatingSummary` | §9.3, §9.4 |
| Verified-purchase count | `RatingSummary.verified_purchase.count` | §9.2 |
| Price minimum, median, maximum, listing count | `price_range[currency]` | §8.3, §10.5 |
| Search position and score | `SearchHit.score`, `why` | §11 |
| Hidden status of a record | `Moderation` | §3.6, §7 |
| Seller restriction | `AuthorView.restriction` | §6 |
| Chosen value of a fact | `Fact<T>` | §10.4 |
| Lowest price in search results | `SearchHit.min_price` | §11.6 |

### T3. Moderated content is labeled and expandable, never silently removed

- A hidden item keeps its place in every list. Its body is collapsed behind a moderation notice (section 6.5).
- The notice states who flagged it, how many flags count, the threshold, the reasons, and whether the author appealed.
- One action shows the content. A second action lists every flag (`client.flags(rootId)`).
- Withdrawn items (retracted) and items whose content a node removed (unavailable) also keep a one-line notice in place.
- List headers state totals including hidden items: "42 reviews, 3 hidden".
- Moderation never changes ratings (7.6). The rating disclosure says so: "Hidden reviews still count in ratings. Hiding collapses their text only."

### T4. Badges have one precise meaning each

- Each badge is defined by one sentence in section 6.2. The badge links to that sentence and to the evidence record.
- Badges name who vouched and until when: "Identity verified by Acme ID until 3 Mar 2027".
- "Verified purchase" means: the seller signed a receipt for this buyer's key, the seller's identity is verified, and the buyer's identity is verified when the policy requires it (the default) (7.5).
- Forbidden labels: "Trusted", "Top", "Recommended", "Best", "Popular", "Official", "Genuine", "Safe", "Sponsored", and star-only seller scores. Perch has no data that backs them.
- A display name is never a badge. Names are chosen by the key's owner. Only `legal_name` in a business attestation is checked by an attestor (4.5).

### T5. Missing data is stated

- "No verified-purchase reviews" is shown as a sentence with the count 0, not as an empty area.
- A missing price says why: "No price: 2 listings, none from a domain-verified seller."
- Unverified authors are labeled "Identity not verified", in neutral color. It states a fact, not a judgment.

### T6. Counts accompany every mean

- A mean never appears without its count: "4.2 from 37 verified-identity reviews".
- The primary rating is the verified-identity group, because ranking uses only it (7.8). The all-reviews group is secondary. The verified-purchase group is shown next to it.
- With 0 reviews in a group, the mean is shown as "No rating" and the histogram is not drawn.
- With 1 to 5 reviews in a group, the histogram shows counts as text, not bars. Bars from so few ratings suggest a distribution that does not exist ([Baymard](https://baymard.com/blog/user-ratings-distribution-summary) recommends hiding the summary at 5 or fewer ratings; Perch keeps the counts visible because they are evidence).

### T7. No paid placement, explained order, no dark patterns

- Perch has no sponsored placement. No field in any view can express one.
- Every sorted list names its order next to the list: "Sorted by relevance. How relevance works". Sort orders are in views §11.7.
- Defaults are the neutral choice: relevance for search, newest first for reviews, view order for listings.
- Forbidden: countdown timers, "only N left" claims, pre-checked boxes that publish or share, confirm-shaming, hidden costs, and nagging. The one pre-checked box is "Keep my key on this device", which section 10 of the contract requires and which publishes nothing.
- Publishing always shows what becomes public and who can see it, before the user signs.

### T8. Build, policy, and source facts stay visible

- The footer shows the app version, the wasm SHA-256, and the active policy hash on every screen (10).
- A banner appears whenever the active policy differs from the bundled default: "Your policy differs from the default in 2 settings. Numbers may differ from what others see."
- A banner appears whenever connected nodes disagree, or only one node is connected (section 7).
- The app never adopts a node's policy or attestor list without the user's action (10).

## 5. Information architecture and navigation

### 5.1 Screens and routes

Routes use the URL fragment (`#/...`). The browser does not send the fragment to the server, so a node that serves the app does not learn which product a user views.

| Screen | Route | Purpose |
|---|---|---|
| Search | `#/search?q=<text>&sort=<order>&currency=<code>` | Find products. Start screen. |
| Product | `#/product/<product key>?reviews=<everyone\|purchase\|identity>&stars=<1-5>` | Everything known about one product. |
| Listing | `#/listing/<root id>` | One seller's offer, its history and moderation. |
| Author | `#/author/<author hex>` | Who an author is, their status, and their records. |
| Publish | `#/publish/<kind>?target=<id>&product=<key>` | Forms for every kind the user can publish. Without `<kind>`: a chooser. |
| Keys | `#/keys` | Create, import, back up, and share the user's key; verification steps. |
| Network | `#/network` | Nodes, sync state, head comparison, build info. |
| Policy | `#/policy` | The rules that compute every view. |
| Data | `#/data`, `#/data/<record id>` | Import, export, and the record inspector. |
| Glossary | `#/help/glossary` | The terms in section 13. |

### 5.2 Persistent layout

```text
┌──────────────────────────────────────────────────────────────┐
│ Perch   [ Search products, brands, or GTIN      ][Search]     │  header
│ Search  Publish  Keys  Network ●  Policy  Data                │  main nav
├──────────────────────────────────────────────────────────────┤
│ banner slot: offline · nodes disagree · one node · policy     │  0 to 2 banners
├──────────────────────────────────────────────────────────────┤
│ breadcrumb                                                    │
│ screen content                                                │
├──────────────────────────────────────────────────────────────┤
│ App 0.1.0 · wasm 3fa9c1d2… · Policy 7be0… (default) · 3 nodes │  footer
└──────────────────────────────────────────────────────────────┘
```

- The header search is on every screen. On Search it is the only search box; the screen does not repeat it.
- The network item shows a status dot with a text equivalent (section 6.7).
- Below 600 px the main nav collapses into a "Menu" button (a disclosure button with `aria-expanded`). Header search stays visible. No horizontal page scroll at 360 px.
- The banner slot shows at most 2 banners, in this priority: offline; nodes disagree; only one node; policy differs from default. Further banners are listed on Network.
- The footer shows build and policy facts on every screen (T8).

### 5.3 Navigation rules

- Every author chip links to Author. Every record link links to `#/data/<id>`.
- Every product name links to Product. Every listing row links to Listing.
- Breadcrumbs: Search › category path › product name › listing title. Category levels come from the category fact and link to a search for that category.
- Search query, sort, currency, and review filters are in the URL, so Back, Forward, and shared links restore them.
- After navigation, focus moves to the screen's `<h1>`, and the live region announces the screen title.
- Actions that publish start from the item they act on: "Flag", "Appeal", "Withdraw", "Edit", and "Issue receipt" open Publish with `target` filled in.

### 5.4 Content priority

On every screen, content appears in this order:

1. What the user came for (product name, price, rating).
2. Its trust state (verification, moderation, conflicts).
3. Evidence (sources, records, disclosures).
4. Actions (publish, flag, export).

At 360 px this order is the reading order. On wide screens, items 1 and 2 may share a row.

## 6. Shared components

### 6.1 Author chip

```text
[▦] Kettle Lab  · Identity verified · c6488cb3…
```

| Part | Content | Rule |
|---|---|---|
| Fingerprint | 5 × 5 glyph from the key (`fingerprint.js`) | Decorative (`aria-hidden`). It helps people notice a changed key. It carries 15 bits and can be copied in about 1.5 s of computing (section 2). |
| Name | `name` from the author's profile, with `<bdi>` | If `null`: "Unnamed author". If the profile is hidden: "Name hidden by moderation". |
| Verification | Badge from section 6.2, or "Identity not verified" | Always present. |
| Status | Forked, revoked, retired, or restricted badge | Only when not `ok` or when restricted. |
| Short key | First 8 hex characters + "…" in the mono font | A reference, not proof (section 2). A "Copy key" button copies all 64 characters. |

- Business authors with a `legal_name` show it after the display name: "Kettle Lab (Kettle Lab GmbH, DE)".
- When the display name differs from the attested `legal_name`, both appear. The name is never replaced by the legal name, so the user sees what the author claims and what was checked.
- The chip is one link to Author. Its accessible name reads the full content: "Kettle Lab, identity verified, key c6488cb3".

### 6.2 Badges

A badge is a short label with an icon and a full sentence. The label appears inline. The full sentence appears on the badge's detail (a disclosure or the Author screen) and in the badge's accessible description.

| Badge label | Full sentence | Shown when | Evidence link |
|---|---|---|---|
| Identity verified | "Identity verified by `<attestor>` until `<date>`. Level: person." | `verified` not null, `level` = `person` | The `attest.identity` record |
| Business verified | "Business `<legal_name>` (`<country>`) verified by `<attestor>` until `<date>` from a company registry." | `level` = `business` | Same |
| Identity not verified | "No attestor in your policy has verified this key." | `verified` is null | — |
| Shop domain verified | "`<attestor>` confirmed until `<date>` that this seller controls `<domain>`." | `domain_verified` | The `attest.domain` record |
| Domain not verified | "No attestor in your policy has confirmed that this seller controls `<host>`. Check the address before you pay." | listing with `domain_verified` false | — |
| Verified purchase | "The seller signed a receipt for this reviewer's key on `<purchased_at date>`. The seller's identity is verified." + "So is the reviewer's." when `verified_purchase_requires_verified_buyer` is true | `purchase` = `verified` | The listing and the receipt fields |
| No receipt | "The review does not include a purchase receipt." | `purchase` = `none` | — |
| Receipt not accepted | Reason sentence from section 6.3 | `purchase` is another value | The receipt fields |
| Conflicting history | "This key signed two different records at position `<fork_seq>`. Its records from that position on are ignored." | `status` = `forked` | Both `fork_evidence` records |
| Key revoked | "`<attestor>` reported this key as compromised. Records after #`<valid_through_seq>` are ignored." | `status` = `revoked` | The `attest.revoke` record |
| Retired | "The owner ended this key at record #`<seq>`. Later records are ignored." + "Successor: `<key>`. Verification does not transfer." when present | `status` = `retired` | The `retire` record |
| Restricted seller | "`<hidden_listings>` of this seller's listings are hidden, flagged by `<distinct_flaggers>` verified people. Policy limits: `<seller_hidden_listing_limit>` listings and `<seller_min_distinct_flaggers>` people." For a fork-based restriction: "This person has a key with a conflicting history." | `restricted` | The Author screen |
| Hidden | Moderation notice (6.5) | `status` = `hidden` | Flags list |
| Withdrawn | "The author withdrew this on `<date>`." | `status` = `retracted` | The `retract` record |
| Removed by node | Redaction notice (6.6) | `status` = `unavailable` | The `redact` record |

- Attestor names come from the attestor's profile. Attestor profiles are names chosen by the attestor, so the Policy screen is where a user decides which attestors to trust; the badge links there.
- Dates use the date format in section 6.9.
- An expired attestation produces no badge. The Author screen states "Identity verification by `<attestor>` expired on `<date>`."

### 6.3 Purchase status text

From `ReviewView.purchase` (7.8, views §9.2):

| Value | Label | Sentence |
|---|---|---|
| `verified` | Verified purchase | As in 6.2. |
| `none` | No receipt | "The review does not include a purchase receipt." |
| `receipt_invalid` | Receipt not accepted | "The receipt's signature does not match the listing's seller, or it names a different buyer." |
| `wrong_product` | Receipt not accepted | "The receipt is for a listing of a different product." |
| `self` | Receipt not accepted | "The reviewer is the seller." |
| `seller_unverified` | Receipt not accepted | "The seller's identity is not verified under your policy." |
| `buyer_unverified` | Receipt not accepted | "The reviewer's identity is not verified, and your policy requires it (`verified_purchase_requires_verified_buyer`)." |

### 6.4 "How this was computed" disclosure

- Element: `<details class="computed">` with `<summary>How this was computed</summary>`.
- Placement: directly after the number it explains, in the same card.
- Closed by default. Opening it never moves focus.
- Content order: section 4, rule T2.
- Policy values are rendered as `name: value` in the mono font, each linking to its row on Policy.
- One level only. Lists inside it (for example excluded reviews) link to their items instead of nesting another disclosure ([Nielsen 2006](https://www.nngroup.com/articles/progressive-disclosure/)).

Example for the rating summary:

```text
How this was computed
  Mean 4.21 from 37 reviews by identity-verified people.
  Not counted: 2 by sellers of this product · 1 withdrawn · 3 older reviews by the same person.
  Hidden reviews still count. Hiding collapses their text only.
  Policy: trusted attestors 2 · verified_purchase_requires_verified_buyer: true
  Rule: one review per person per product, latest version. views §9.3
  Policy 7be0c2a1… (copy) · computed at 18 Oct 2026, 14:05
```

### 6.5 Moderation notice

Used for every item with `status` = `hidden`: listings, reviews, products, and profiles.

```text
┌ ⚑ Hidden by moderation ────────────────────────────────────────┐
│ 5 verified people flagged this review. Your policy hides        │
│ reviews at 5 counted flags. Reasons: spam 4, misleading 1.      │
│ The author appealed on 12 Oct 2026.                             │
│ [Show review]  [Show flags (7)]                                 │
└─────────────────────────────────────────────────────────────────┘
```

| Part | Source |
|---|---|
| Counted flags and threshold | `flags.counted`; `threshold(kind)` from the policy (views §2.1) |
| Reasons | `flags.reasons`, sorted by count descending, then reason |
| Uncounted flags | `flags.count − flags.counted`: "2 more flags do not count. Show why." |
| Appeal | `appeal` (date and full note, with `<bdi>` and `dir="auto"`) |
| Flags list | `client.flags(rootId)`: each flagger's chip, reason, note, date, and "Counts" or the excluded reason |

- "Show review" expands the content in place. The notice stays above it. Expanded state is per item and resets on navigation.
- Flag exclusion reasons use this text: `retracted` "withdrawn by the flagger"; `unavailable` "content removed by a node"; `superseded` "replaced by a newer flag from the same key"; `forked` "flagger's key has a conflicting history"; `self` "flagged by the author's own person"; `product_seller` "flagged by a seller of this product"; `unverified` "flagger's identity is not verified, and your policy counts only verified flags"; `duplicate_person` "same person already flagged".
- Items with flags below the threshold show a one-line note, not a notice: "2 flags (spam). Not hidden: your policy hides at 3."
- A product with `moderation.status` = `hidden` keeps its facts, listings, and reviews visible, because other authors share the product key. The notice explains: "The product record is hidden. Listings and reviews for this product are shown."

### 6.6 Redaction notice

Used for every item with `status` = `unavailable`.

| Case | Text |
|---|---|
| Notice published (`redaction.record` not null) | "Content removed by node operator `<by chip>` on `<date>`. Reason: `<reason label>`. `<note>`" |
| No notice | "The content of this record is not available from your connected nodes. No removal notice was published." |

- Reason labels: `illegal` "Illegal content"; `personal_data` "Personal data"; `court_order` "Court order"; `other` "Other".
- The notice keeps the record's header facts: author chip, stated date, kind, and record link.
- Unavailable items do not count in any aggregate (5.3). The aggregate's disclosure lists them.
- Action: "Ask other nodes" connects to the nodes on Network that are not yet connected and requests the body (sync `want_bodies`). If the body arrives and its hash matches, the item renders normally.

### 6.7 Status indicators

Every color cue has a text or icon equivalent (WCAG 1.4.1).

| State | Icon | Text | Token |
|---|---|---|---|
| Good (verified, live, matches) | Check or shield | Required | `--good` |
| Warning (domain not verified, nodes disagree, single node) | Triangle | Required | `--warn` |
| Bad (forked, revoked, restricted, signature fails) | Octagon or ban | Required | `--bad` |
| Neutral (not verified, retired, withdrawn) | Circle | Required | `--muted` |
| Hidden | Flag | Required | `--hidden-tint` background |

Network dot in the nav: green circle "connected to N nodes"; amber triangle "nodes disagree" or "1 node"; grey circle "offline". The text is in the link's accessible name.

### 6.8 Record link and key display

- Record IDs and keys use the mono font (`--mono`). Atkinson Hyperlegible Mono distinguishes `0`/`O` and `1`/`l`.
- Short form: first 8 hex characters + "…". Full form: 64 characters in 4 groups of 16, separated by a thin space (U+2009) for reading. Copy buttons copy the ungrouped value.
- A record link reads "Record 25838fcd…" and opens the record inspector.
- "Compare" on Author shows the full key next to a text field where the user can paste a key; it reports "Same key" or "Different key: differs at character 9".

### 6.9 Numbers, money, and dates

| Value | Rule | Example (en-US / de-DE) |
|---|---|---|
| Money | Get minor units from `client.currencies()`. Build a decimal string from the integer: `"12995"` with 2 minor units → `"129.95"`. Format the string with `Intl.NumberFormat(locale, { style: "currency", currency, minimumFractionDigits: m, maximumFractionDigits: m })`. Never divide in floating point. Never take fraction digits from `Intl`. | 12995 USD → "$129.95" / "129,95 $"; 1500 JPY → "¥1,500"; 1234567 BHD → "BHD 1,234.567" |
| Money input | Parse the user's text with the same table. Accept the locale's decimal separator and grouping. Reject more fraction digits than the currency has. | "129,95" in de-DE, EUR → 12995 |
| Mixed currencies | Never convert or combine. Group by currency. Show the ISO code in group headers: "Prices in EUR". | — |
| Mean | Show the view's value with `minimumFractionDigits: 1, maximumFractionDigits: 2`. It is already rounded (7.7). | 4.25 → "4.25" / "4,25"; 4 → "4.0" |
| Counts | `Intl.NumberFormat` integer; words via `Intl.PluralRules` | "1,284 reviews" |
| Dates | `Intl.DateTimeFormat(locale, { dateStyle: "medium" })`. Add `timeStyle: "short"` where minutes matter (price changes). | "18 Oct 2026" |
| Exact time | ISO 8601 in UTC, in the record inspector and in `title` attributes | "2026-10-18T14:05:00.000Z" |
| Relative time | Not used. Views compute at `policy.now`, and relative times change while the page is open. | — |
| Hash and key | Lowercase hex, mono font, never localized | — |

Test vectors for money (computed in Node 22, ICU):

| amount | currency | locale | Expected |
|---|---|---|---|
| 12995 | USD | en-US | `$129.95` |
| 12995 | USD | de-DE | `129,95 $` (U+00A0 no-break space before $) |
| 12995 | EUR | fr-FR | `129,95 €` (U+00A0 no-break space before €) |
| 1500 | JPY | ja-JP | `￥1,500` |
| 1500 | IQD | en-US | `IQD 1.500` (ISO 3 minor units; `Intl` default would give 0) |
| 5 | USD | en-US | `$0.05` |
| 9007199254740991 | USD | en-US | `$90,071,992,547,409.91` (string input; Number input gives `.90`) |

## 7. Global states

Every screen handles these states. Screen specs in section 8 add screen-specific text.

| State | Detected by | What the user sees |
|---|---|---|
| Starting | wasm module not yet loaded | "Loading the Perch client." in `<main>`. No spinner animation longer than 5 s without text change; after 5 s add "This can take longer on slow connections." |
| Client failed | `PerchClient.create()` throws, or WebAssembly is unavailable | Error page: "Perch could not start on this device. It needs a browser with WebAssembly. Error: `<message>`." Action: "Try again". No partial data. |
| No data | `stats().records` = 0 | "No records on this device yet." Action: "Connect to a node" (Network). If nodes are connecting: "Receiving records from 2 nodes." |
| First sync | any peer in state `Syncing` | Content renders as records arrive. Banner: "Still receiving records from 2 nodes. Numbers may change." Screens re-render on `onChange`, at most once per 500 ms, without moving focus or scroll position. |
| Offline | `navigator.onLine` false, or every peer `Closed` | Banner: "Offline. Showing the 12,340 records stored on this device. Records you publish are saved here and sent when a node connects." |
| One node | exactly 1 peer `Live` | Banner: "Connected to 1 node. Perch cannot check whether it is leaving records out. Add another node." Link: Network. |
| Nodes disagree | any `Live` peer with `behind` > 0 or `ahead` > 0 for more than 30 s | Banner: "Connected nodes hold different data. Node node-b.example lacks records for 12 authors." Link: Network. |
| Policy differs | active policy hash ≠ bundled default hash | Banner: "Your policy differs from the default in 2 settings. Numbers may differ from what others see." Link: Policy. |
| Not found | a view returns `null` | Screen-specific text (section 8) plus "This device holds 12,340 records. Connect more nodes to find more." |

Rules:

- Views are synchronous. No screen shows a loading spinner for a view call.
- `policy.now` is the device time when the screen renders, refreshed every 60 s and on navigation. Disclosures show it (T2).
- Banners are `role="status"` regions. They never take focus. Each has a "Hide for this session" button.

## 8. Screen specs

Each screen lists its content in priority order (section 5.4), then its states.

### 8.1 Search

Purpose: find a product by name, brand, category, MPN, or GTIN (views §11).

Content, in order:

1. **Query.** The header search box. Submitting updates `#/search?q=`.
2. **Controls row.**
   - Sort (`<select>`, label "Sort by"): "Relevance" (`relevance`, default), "Lowest price" (`price`), "Most verified-identity reviews" (`rating_count`), "Highest rating" (`rating_mean`).
   - Currency (`<select>`, label "Price currency"): shown only when hits have prices in more than one currency. Default: the view's sort currency (views §11.6). Passed as `options.currency`.
3. **Result line.** "24 products match "kettle". Sorted by relevance." followed by the disclosure "How relevance works": the score formula in plain words, the constants (views §13), and "Ranking uses only reviews from identity-verified people. No one can pay for position."
4. **Hits**, in the order the view returns them. Each hit shows:

   | Priority | Content | Source |
   |---|---|---|
   | 1 | Product name (link), brand | `name`, `brand` |
   | 2 | Rating line: "4.2 from 37 verified-identity reviews" or "No verified-identity reviews" | `rating_mean`, `rating_count` |
   | 3 | Purchase line: "12 verified purchases, mean 4.4" or "No verified purchases" | `verified_purchase_count`, `verified_purchase_mean` |
   | 4 | Price: "Lowest €89.00" or "No current price" | `min_price` |
   | 5 | Category path, GTIN or MPN | `category`, `key` |
   | 6 | Disclosure "Why this position" | `score`, `why` |

5. **Moderated hits.** Hits with `moderation_status` other than `visible` come after all visible hits (7.8). They are collapsed into one group: "2 more products match but their product records are hidden by moderation. Show them." Each shows its status label (section 6.2) and links to Product, where the notice explains why.

"Why this position" turns each `why` string into a sentence:

| `why` pattern | Sentence |
|---|---|
| `name exact "kettle" +30` | Name contains "kettle": +30 |
| `brand prefix "ke" +10` | Brand has a word starting with "ke": +10 |
| `gtin exact "4006381333931" +100` | GTIN matches: +100 |
| `rating 4.21 from 12 verified-identity reviews +12.1` | Rating 4.21 (adjusted for review count) from 12 verified-identity reviews: +12.1 |
| `has price +5` / `no price +0` | Has a current price: +5 / No current price: +0 |

The last line reads "Score: 47.1". Numbers are localized; the raw `why` strings are available under "Show raw".

GTIN input: when the query is 8, 12, 13, or 14 digits, the result line adds "Searching by GTIN." If the check digit fails: "4006381333932 is not a valid GTIN: the last digit should be 1. Showing text matches."

States:

| State | Text |
|---|---|
| Empty query | "Search by product name, brand, or GTIN (the number under the barcode)." Below: "This device holds records about 1,020 products from 312 authors." |
| No hits | "No products match "kettel"." Then: "Check the spelling, search by brand, or search by GTIN. This device holds records about 1,020 products; connect more nodes to search more data." |
| Only moderated hits | "No visible products match. 1 product with a hidden product record matches." The group is expanded. |
| First sync, offline, one node | Global banners (section 7). |

### 8.2 Product

Purpose: everything known about one product key, with sources. Route: `#/product/<key>`.

Content, in order:

1. **Header.**
   - Name (`name.value`, `<h1>`, `<bdi>`). If `name.conflicts` is not empty: "Other names: 2" opens the fact detail.
   - Brand, then identifiers: GTIN in printed form (below), MPN.
   - Breadcrumb from the category fact.
   - GTIN printed form: from the 14 digits, show the last 8 when the first 6 are `0`; else the last 12 when the first 2 are `0`; else the last 13 when the first is `0`; else all 14. This matches the number printed under most barcodes. A GTIN-12 that starts with 0000 also prints as 8 digits under this rule; search accepts every form.
2. **Product moderation notice**, when `moderation.status` is not `visible` (section 6.5).
3. **Summary card.**
   - Rating from identity-verified reviewers, with count (T6).
   - Verified-purchase count and mean.
   - Lowest current price per currency, with listing count: "Lowest €89.00 from 5 listings".
   - Links: "Reviews", "Listings", "Price history".
4. **Images.** `images` in view order. Images whose `author_verified` is false show a placeholder: "Image from cdn.shop.example. Loading it shows your IP address to that host. [Load image]" (10). When `sha256` is present, the app fetches the image, checks the hash, and shows it from a `blob:` URL; on mismatch: "The image at cdn.shop.example does not match its stated hash. Not shown." If the fetch is blocked (CORS), the app shows the image without the check and labels it "Hash not checked".
5. **Facts.** A definition list: brand, category, description, then each attribute in key order.
   - Each value shows "1 source" or "N sources" and the first source's author chip.
   - Values with conflicts show "Other values: N". The detail lists every value with its sources, ranked by the rule in views §10.4: "Values are ranked by the number of verified people who state them, then by all people, then alphabetically."
   - Description: first 6 lines, then "Show full description".
   - `excluded_sources` with any count > 0: "Not used: 1 hidden product record, 1 withdrawn." with links.
6. **Prices.** One card per currency in `price_range`, ordered by `count` descending, then code:
   "EUR: lowest €89.00 · median €99.00 · highest €129.00 · 5 listings." Disclosure (T2): feeding rules (visible, in stock or preorder, seller not restricted, shop domain verified if `price_requires_domain_verified`), `excluded_unverified_domain` ("2 listings not counted: shop domain not verified"), "Median of an even count is the lower middle value." and "Prices in different currencies are never converted."
7. **Listings.** Header: "8 listings: 5 shown, 2 hidden, 1 withdrawn". In view order (views §8.4).

   | Column (wide) | Content |
   |---|---|
   | Seller | Author chip; "Restricted seller" badge if `seller_restricted` |
   | Price | Money; "Not in price summary: `<reason>`" when the listing does not feed prices |
   | Availability, condition | Labels from section 12.3 |
   | Ships to | Country names via `Intl.DisplayNames`; more than 3: "Germany, Austria, and 12 more" |
   | Shop | Host in ASCII form + "Shop domain verified" or "Domain not verified" badge |
   | Updated | Stated date of the latest version |

   - Below 600 px each listing is a card with the same fields in the same order.
   - Hidden listings: moderation notice in place of the row content. Withdrawn: one line "Withdrawn by the seller on `<date>`". Unavailable: redaction notice.
   - Filter "Ships to" (`<select>` of countries): a display filter. It changes the visible rows only. Text next to it: "Price summary above includes all listings."
8. **Price history.** One chart per currency (section 10.6). Above it: "Showing 3 of 5 listings. Choose listings" opens a checkbox list (at most 3). Default: the first 3 price-feeding listings in view order. Below: "Show as table" with every point. Caption: "Dates are those stated by sellers and observers. They are not checked. Observations count only from identity-verified authors."
9. **Rating summary.** Three groups:

   | Group | Field | Position |
   |---|---|---|
   | Identity-verified reviewers | `verified_identity` | First, largest; its histogram is drawn |
   | Verified purchases | `verified_purchase` | Second |
   | All counted reviews | `count`, `mean`, `histogram` | Third, with "Includes reviewers whose identity is not verified" |

   - Each group: mean, count, histogram (section 10.6), and a T2 disclosure.
   - Histogram rows are buttons that set the star filter below (Baymard). With 1 to 5 reviews in a group: counts as text, no bars (T6).
   - `excluded` line: "Not counted: 2 by sellers of this product, 1 withdrawn, 3 older reviews by the same person, 1 removed by a node."
   - Always shown: "Hidden reviews still count in ratings. Hiding collapses their text only."
10. **Reviews.** Header: "64 reviews · 3 hidden · newest first".
    - Filter 1 (radio group, label "Show reviews from"): "Everyone (64)", "Verified purchases (12)", "Identity-verified people (37)".
    - Filter 2 (radio group, label "Stars"): "All", "5", "4", "3", "2", "1", each with its count under filter 1.
    - Order: the view's order (newest first). No other order in v1, because ordering is logic that belongs in Rust (D8).
    - Show 20, then "Show 20 more". No infinite scroll.
    - Each review shows, in order: star rating with number ("4 out of 5"), title, author chip, purchase badge (6.3), stated date, text (`dir="auto"`, first 8 lines then "Show full review"), "Bought from: `<seller>` listing" when `listing` is set, and actions ("Flag", "Record").
    - `counted` false: "Not counted in ratings: `<reason>`" (section 12.3).
    - Hidden: moderation notice in place of title and text; rating, author, and date stay visible, because the rating still counts.
    - Withdrawn: "A review by `<chip>` was withdrawn on `<date>`."
    - Unavailable: redaction notice.
11. **Actions.** "Write a review", "Add a listing", "Report a price you saw" (observation), "Flag the product record", "Records for this product" (Data inspector list of sources).

States:

| State | Text |
|---|---|
| Not found | "No records about `<printed GTIN or key>` on this device." Actions: "Add this product" (Publish product, GTIN filled in), "Network". |
| No listings | "No listings for this product." Action: "Add a listing". |
| No reviews | "No reviews yet." Action: "Write a review". Rating summary shows "No rating" for each group. |
| No price | "No current price. 2 listings exist; none is in stock from a seller with a verified shop domain." (built from the exclusion counts) |

### 8.3 Listing

Purpose: one seller's offer, its history, and its moderation. Route: `#/listing/<root id>`.

Content, in order:

1. **Header.** `title` or the product name (`<h1>`, `<bdi>`); product link; seller chip with status badges.
2. **Moderation notice**, when hidden. Withdrawn and unavailable notices replace sections 3 to 5.
3. **Offer.** Price (large), availability, condition, ships to, location (country and region), "Updated `<stated date>`", "Edited N times" (link to versions in the inspector).
4. **Shop link.**
   - Text: "Go to shop: `<host>`" plus "(opens in a new tab)" for screen readers. The host is the ASCII (punycode) form from perch-core (4.5).
   - `rel="noopener noreferrer nofollow external"`, `referrerpolicy="no-referrer"`.
   - Domain verified: badge sentence from 6.2 next to the link.
   - Domain not verified: a warning notice directly above the link: "`<host>` is not verified as this seller's shop. The link may lead to a copy of a real shop. Check the address in your browser before you enter payment details."
5. **Price history.** Chart with 2 series: seller-stated prices (step line, circles) and verified observations (diamonds). Table view. Same caption as 8.2.
6. **Flags.** When `flags.count` > 0: the counts line, then the flags list (`client.flags(id)`), then the appeal note if any. Shown even when the listing is visible.
7. **Receipts from this seller.** From `AuthorView.seller_reviews`: "41 of 120 counted reviews of this seller's products cite a valid receipt from this seller." Disclosure: "Sellers choose whom to give receipts. A low share can mean receipts are given selectively (threat-model R8)."
8. **Reviews citing this listing.** Reviews of the product with `listing` = this root ID, rendered as in 8.2.
9. **Actions.**
   - Everyone: "Flag this listing", "Report a price you saw", "Record".
   - Seller (current key is `seller`): "Edit", "Issue a receipt", "Appeal" (when flagged), "Withdraw".

States: not found ("No listing with ID `<short id>` on this device."); seller forked, revoked, or retired (badge sentence at the top; the listing renders).

### 8.4 Author

Purpose: who an author is, their status, and their records. Route: `#/author/<hex>`.

Content, in order:

1. **Header.** Display name (`<h1>`, `<bdi>`) or "Unnamed author"; role label; fingerprint; full key (grouped) with "Copy key" and "Compare"; website link (host shown); about text (first 6 lines).
   - Profile hidden: "Name hidden by moderation" in the `<h1>` and the moderation notice for `profile`.
   - "This is your key" note when it matches the current key, with a link to Keys.
2. **Status.** One block, shown only when `status` ≠ `ok` or `restricted`:
   - Forked: badge sentence + links to both `fork_evidence` records + "Records from #`<fork_seq>` on are ignored."
   - Revoked: badge sentence + attestor chip + `valid_through_seq`.
   - Retired: badge sentence + successor chip (if any) + "Verification and history do not transfer to the successor."
   - Restricted: badge sentence with `restriction.hidden_listings` and `restriction.distinct_flaggers` and the policy limits.
3. **Verification.**
   - Identity: badge sentence; `legal_name` and country for businesses; attestor chip; expiry date.
   - Domains: one line per entry in `domains`.
   - None: "No attestor in your policy has verified this key." For the user's own key: link to Keys, section "Get verified".
4. **Seller summary** (when the author has listings): listing count, hidden listing count, and the receipts line from 8.3 item 7.
5. **Records.** Tabs (ARIA tabs pattern): Listings, Reviews, Products, Observations, Flags, Other. Each tab calls `client.authorRecords(author, { kinds, limit: 50, from_seq })` and shows `ChainSummary` rows: title or product, kind, stated date, status label. "Show 50 more" pages with `from_seq`.
   - `status` = `invalid`: "Ignored by views: `<reason>`" from `client.recordStatus(latest)`, with reason text from section 12.3.
6. **Log.** "`<record_count>` records in this author's log." Action: "Download log (JSONL)" from `client.authorLog(author)`.

States: not found ("No records by this key on this device."); key with only attestations about it ("This key has published nothing. 1 attestation names it.").

### 8.5 Publish

Purpose: create, edit, and withdraw the user's own records. Route: `#/publish/<kind>`.

#### 8.5.1 Before any form: publish gate

The gate runs when Publish opens and again on "Sign and publish". It shows the first unmet condition as a notice at the top of the form. The form stays editable, so the user can prepare a record while the gate resolves.

| Order | Condition | Notice | Action |
|---|---|---|---|
| 1 | `client.author()` is null | "You need a key to publish. Your key signs everything you publish." | "Create or import a key" (Keys) |
| 2 | Backup not confirmed for this key (10) | "Confirm your key backup before your first record. Without a backup, a lost key cannot be recovered." | "Back up key" (Keys) |
| 3 | `publishStatus().ready` is false | "Checking your latest record on each node, so you do not publish over it." + one line per node from `nodes`: "node-a.example: done, record #42", "node-b.example: waiting". If `reason` is set, show it. | "Network" |

- The "Sign and publish" button is never disabled. If pressed while the gate is unmet, the error summary repeats the gate notice and focuses it. Disabled buttons give no reason and cannot be focused.
- Offline publishing works. The success message then says "Saved on this device. It will be sent when a node connects."

#### 8.5.2 Form pattern

1. **Chooser** (`#/publish`): Profile, Product, Listing, Price observation, Review, Flag, Appeal, Withdraw. Each with one sentence: what it is and who sees it.
2. **Form.** Labels above fields. Hints below labels. Optional fields marked "(optional)"; required fields unmarked. Character counters on text fields with limits: "120 of 2,000 characters". Counters count Unicode scalar values (`[...s].length`), as 4.5 does.
3. **Validation** on "Review" (not on each key press). Errors appear in an error summary at the top (links to each field, focus moves to the summary) and next to each field. All input is kept.
4. **Review step.** "This will be public" lists every field as it will be signed, the author key, and the stated time. "Show record JSON" shows the exact body. Sentence: "Anyone can copy this record. You can withdraw it later, but copies remain, marked withdrawn."
5. **Sign and publish.** Result: "Published. Record 25838fcd…" with links to the item and the record. Then a status line from sync: "Sent to 2 of 3 nodes."

Editing: "Edit" opens the form filled from the latest version and publishes with `replaces`. Fixed fields (4.5) are read-only with the hint "`<Field>` cannot change after publishing. To change it, withdraw this record and publish a new one."

#### 8.5.3 Validation messages

Messages follow section 12.2. Rules come from 4.5 and views §4.1. Lengths count Unicode scalar values.

| Form | Field | Rule | Message |
|---|---|---|---|
| any | text with limit N | > N | "`<Field>` must be N characters or fewer" (N localized: "2,000") |
| any | required text | empty | "Enter `<field in lower case>`", for example "Enter a name" |
| any | URL field | not parsable | "Enter a web address like https://shop.example/item" |
| any | URL field | scheme not `https` | "`<Field>` must start with https://" |
| any | URL field | user name or password | "`<Field>` must not contain a user name or password" |
| any | URL field | IP address host | "`<Field>` must use a domain name, not an IP address" |
| any | URL field | > 2,048 | "`<Field>` must be 2,048 characters or fewer" |
| any | URL field, Unicode host | parsed | Not an error. Hint below the field: "Shown to others as `<ASCII host>`" |
| Profile | Name | 1–100 | "Enter a name" / "Name must be 100 characters or fewer" |
| Profile | Role | one of 7 | "Select a role" |
| Profile | About | ≤ 2,000 | "About must be 2,000 characters or fewer" |
| Product | Name | 1–300 | "Enter the product name" / "Product name must be 300 characters or fewer" |
| Product | Brand | 1–100 | "Enter the brand" / "Brand must be 100 characters or fewer" |
| Product | GTIN | digits only | "GTIN must contain only digits 0 to 9" (spaces are removed before checking) |
| Product | GTIN | length 8, 12, 13, 14 | "GTIN must have 8, 12, 13, or 14 digits. You entered 11." |
| Product | GTIN | check digit | "The last digit of this GTIN should be 1. Check the number under the barcode." |
| Product | GTIN | product key exists | Not an error. Notice: "A product with this GTIN already exists. Your record will add to it." + link |
| Product | MPN | 1–100 | "MPN must be 100 characters or fewer" |
| Product | Category | ≤ 200, lowercase, `/`-separated | "Category must use lowercase words separated by /, like kitchen/kettles" |
| Product | Attributes | ≤ 64 entries | "A product can have at most 64 attributes" |
| Product | Attribute name | 1–64 | "Enter an attribute name" / "Attribute name must be 64 characters or fewer" |
| Product | Attribute name | duplicate | "Attribute names must be different. "Weight" appears twice." |
| Product | Attribute value | ≤ 500 | "Attribute value must be 500 characters or fewer" |
| Product | Images | ≤ 16 | "A product can have at most 16 images" |
| Product | Image hash | 64 lowercase hex | "Image hash must be 64 characters, 0 to 9 and a to f" |
| Product | Description | ≤ 5,000 | "Description must be 5,000 characters or fewer" |
| Listing | Product | ProductKey | "Choose the product this listing is for" (product picker: search or GTIN) |
| Listing | Shop link | URL rules | As "any URL field" |
| Listing | Shop link | host not in the seller's `domains` | Not an error. Warning: "shop.example is not verified as your shop domain. This listing will not count in price summaries until a domain attestor verifies it. How to verify" (Keys) |
| Listing | Currency | in `client.currencies()` | "Select a currency" |
| Listing | Price | empty | "Enter a price" |
| Listing | Price | not a number | "Price must be a number, like 129.95" (example uses the locale's separator and the currency's digits) |
| Listing | Price | too many decimals | "Prices in JPY have no decimals" / "Prices in EUR have at most 2 decimals" |
| Listing | Price | > 2^53 − 1 minor units | "Price is too large" |
| Listing | Price | 0 | Not an error. Warning: "The price is 0. Check that this is correct." |
| Listing | Availability | one of 4 | "Select availability" |
| Listing | Condition | one of 3 | "Select the condition" |
| Listing | Title | ≤ 300 | "Title must be 300 characters or fewer" |
| Listing | Ships to | ≤ 250 alpha-2 | "Select at most 250 countries" (country picker shows names, stores codes) |
| Listing | Region | ≤ 100 | "Region must be 100 characters or fewer" |
| Observation | Shop link, price, currency | as Listing | As Listing |
| Observation | Date seen | ≤ now | "The date you saw the price must be today or in the past" |
| Review | Rating | 1–5 | "Select a rating from 1 to 5 stars" |
| Review | Review text | 1–10,000 | "Enter your review" / "Review must be 10,000 characters or fewer" |
| Review | Title | ≤ 200 | "Title must be 200 characters or fewer" |
| Review | Receipt | see 8.5.4 | see 8.5.4 |
| Flag | Reason | one of 7 | "Select a reason" |
| Flag | Note | ≤ 2,000 | "Note must be 2,000 characters or fewer" |
| Appeal | Note | 1–2,000 | "Enter your appeal" / "Appeal must be 2,000 characters or fewer" |

Messages for checks that pass validation but change the outcome (warnings) use the warning notice style and do not block publishing.

#### 8.5.4 Review with a receipt

The review form has an optional "Purchase receipt" section:

- Field: "Paste the receipt the seller gave you" (multi-line text). Paste works (WCAG 3.3.8). Also "Load from file" and, where `BarcodeDetector` exists, "Scan QR code".
- On paste or change, the app parses the JSON and calls `client.verifyReceipt(receipt)`, then predicts the `purchase` value with the rules of 7.5.
- Result panel:

| Result | Style | Message |
|---|---|---|
| Not JSON, or wrong fields | error | "Paste the whole receipt. It starts with { and contains listing, buyer, purchased_at, nonce, and sig." |
| Listing not on this device | error | "The listing in this receipt is not on this device. Connect to more nodes, or check that you pasted the whole receipt." |
| Signature fails | error | "This receipt is not signed by the listing's seller. Ask the seller for a new receipt." |
| `buyer` ≠ current key | error | "This receipt is for key a1b2c3d4…, not your key c6488cb3…. Use the key you gave the seller, or ask the seller for a receipt for this key." |
| Different product | error | "This receipt is for `<other product name>`." Action: "Review that product instead". |
| Seller is the current key's person | error | "This receipt is from your own listing. It cannot make your review a verified purchase." |
| Seller not verified | warning | "The receipt is valid, but the seller's identity is not verified under your policy. Your review will show "Receipt not accepted" until it is." |
| Current key not verified, and policy requires it | warning | "The receipt is valid, but your identity is not verified. Your review will become a verified purchase after an attestor verifies your key." Link: Keys. |
| All rules pass | success | "Valid receipt from `<seller chip>` for `<listing title>`, bought on `<date>`. Your review will show "Verified purchase"." |

- Errors block publishing with that receipt; the user can remove the receipt and publish without it.
- Privacy sentence, always shown in the receipt section: "Adding the receipt shows everyone that your key bought from this listing on `<date>`. It does not show the price or your order number."

#### 8.5.5 Flag

- Target card first: what is flagged (kind, title, author chip, status).
- Reason (radio group), each with one sentence:

  | Reason | Label | Sentence |
  |---|---|---|
  | `fraud` | Fraud | "A scam, or the seller does not deliver." |
  | `counterfeit` | Counterfeit | "The product is a fake of another brand." |
  | `misleading` | Misleading | "False or misleading claims." |
  | `spam` | Spam | "Advertising or unrelated content." |
  | `illegal` | Illegal | "Illegal content. Flags hide it in Perch; to remove it from a node, also contact the node operator." |
  | `personal_data` | Personal data | "Contains someone's personal data. To remove it from a node, also contact the node operator." |
  | `other` | Other | "Describe the problem in the note." |

- Before signing, the review step states the effect under the active policy:
  - Counts: "Your flag will count. This review is hidden at 5 counted flags; it has 3."
  - Does not count: the matching sentence from section 6.5, for example "Your flag will be public but will not count: your identity is not verified, and your policy counts only verified flags."
  - Kind that cannot be hidden: "Flags on `<kind>` records are recorded but hide nothing (7.6)."
- "Your flag is public and signed with your key. The author can see it and appeal."

#### 8.5.6 Appeal

- Available only for the user's own items with at least one flag.
- Hint: "An appeal is public. It does not unhide the item by itself. Flaggers can read it and withdraw their flags."
- Editing an appeal replaces it (the newest version is shown).

#### 8.5.7 Withdraw

- Available for the user's own records except `retract`, `retire`, `attest.revoke`, and `redact`.
- Confirmation page: "Withdraw `<title>`? This withdraws every version of it. You cannot undo this. The record stays in the data, marked withdrawn, and no longer counts."
- Button: "Withdraw". Secondary: "Cancel". No pre-checked boxes.

#### 8.5.8 Issue a receipt (sellers)

Opened from the seller's own Listing. A receipt is not published; it is given privately to the buyer (4.7).

- Fields: "Buyer's Perch key" (paste, or scan QR); "Purchase date" (date input, default today).
- Input is trimmed and lowercased before checking.
- Messages: "Enter the buyer's Perch key: 64 characters, 0 to 9 and a to f"; "Buyer's key has 63 characters. A key has 64."; "This is your own key. A receipt to yourself never makes a verified purchase."; "The purchase date must be today or in the past".
- Output: the receipt JSON (canonical, about 394 bytes) in a read-only field with "Copy receipt", "Download", and a QR code drawn locally. 394 bytes need QR version 15 at error correction M (capacity 412 bytes in byte mode).
- Sentence: "Give this receipt to the buyer. It stays private until the buyer adds it to a review. Perch does not store it."
- Sellers with many orders use a checkout plugin instead (verified-purchase.md 4.1).

### 8.6 Keys

Purpose: the user's identity. Route: `#/keys`.

#### 8.6.1 No key

- Heading: "Your key".
- Text: "A key is your identity in Perch. It signs everything you publish. Perch has no accounts and cannot recover a lost key."
- Primary: "Create a key". Secondary: "Import a backup".
- Note for R12: "Create keys only in the official Perch release. An app served by an unknown node could read keys stored in it." with the current origin shown: "This app runs at `<origin>`."

#### 8.6.2 Create

1. Checkbox "Keep my key on this device", checked by default (10). Hint: "Stored in this browser for `<origin>`. Turn this off on a shared computer. Without it, the key is lost when you close the page, unless you import your backup."
2. "Create key". Then the backup step starts at once.

#### 8.6.3 Backup

- "Download backup" (file `perch-key-<first 8 hex>.json`) and "Copy backup".
- Warning notice: "Anyone with this backup can publish as you. Store it like a password, somewhere other than this browser."
- Confirmation: checkbox "I stored the backup outside this browser", then "Confirm backup". The checkbox is enabled only after a download or copy in this session. Confirmation is per key.
- After publishing: "Your backup records your latest record (#42). Older backups still work: before publishing, the app checks your latest record on the nodes." Action: "Download a new backup".

#### 8.6.4 Import

- Input: file, or paste of the backup JSON or a 64-character secret key.
- Messages: "Paste a Perch backup or a 64-character secret key"; "The secret key has 63 characters. It needs 64."; "This file is not a Perch backup. It should contain "secret"."
- After import: the publish gate progress (8.5.1), one line per node, until `publishStatus().ready`.
- Notice: "Using one key on two devices at once can create a conflicting history. Use a second key for a second device, verified for the same person (identity.md 6.7)."

#### 8.6.5 Key present

Content, in order:

1. Public key, full, grouped, with "Copy" and "Show QR code". Heading: "Your public key". Text: "Give this to a seller at checkout, so they can issue you a receipt. It is public. It cannot be used to publish as you."
2. Identity status (badge sentence) and domain status. If not verified: "Get verified" (8.6.6).
3. Storage: the "Keep my key on this device" checkbox; changing it announces the result (for example "Key removed from this browser's storage. It is lost when you close the page unless you have a backup.").
4. Backup status: "Backup confirmed on 18 Oct 2026" or the backup step.
5. Your records: count and link to your Author page.
6. Danger zone (collapsed): "Remove key from this device", "Retire this key", "My key was stolen".

#### 8.6.6 Get verified

- Identity: "An identity attestor checks who you are, off Perch, and publishes an attestation for your key. Attestors in your policy:" list with chips, website links, and kinds. "What the attestor sees and publishes" links to identity.md 3.2 and 8.
- Domain (sellers): field "Your shop's domain" → the app shows both proofs with the user's key filled in, each with "Copy" (identity.md 3.4):

  ```json
  { "perch": 1, "authors": ["<your key>"] }
  ```

  served at `https://<domain>/.well-known/perch.json`, or a DNS TXT record at `_perch.<domain>` with value `perch-author=<your key>`. Then: "Ask a domain attestor in your policy to check it. Verification lasts at most 90 days and is renewed while the proof stays in place."
- Marketplace hosts: "Listings on marketplaces such as amazon.com cannot be domain verified by sellers (threat-model R5)."

#### 8.6.7 Retire and stolen key

- Retire: optional successor key field; confirmation: "Retiring ends this key's log. Later records are ignored. Verification and reviews do not move to the successor." Button "Retire key".
- Stolen key: steps from identity.md 6.3: contact your identity attestor; it publishes a revocation; create a new key; get it verified for the same identity. "Until the revocation is published, your other keys may be marked restricted if the thief creates a conflicting history (threat-model R7)."

### 8.7 Network

Purpose: where data comes from and whether sources agree. Route: `#/network`.

Content, in order:

1. **Summary sentence.** "Connected to 3 of 4 nodes. 2 hold the same data as this device. node-b.example lacks records for 12 authors."
2. **Nodes table.**

   | Column | Content |
   |---|---|
   | Node | Host and path; full URL in the accessible name |
   | State | `AwaitHello` "Connecting", `Syncing` "Receiving", `Live` "Up to date", `Closed` "Disconnected" (sync-protocol.md 4.1) |
   | Lacks | `behind`: "12 authors" — the node has older records than this device for 12 authors |
   | Has more | `ahead`: "3 authors" — the node has newer records for 3 authors |
   | Sent, received | Record counts |
   | Actions | "Disconnect" or "Connect"; "Remove" |

   Below 600 px each node is a card. Disagreement explanation under the table: "A node that lacks records may be slow, or may be leaving records out. This device sends its records to nodes that lack them. If a node keeps lacking records after a few minutes, it may be filtering data."
3. **Add a node.** Field "Node address", hint "Like wss://node.example/v1/sync". Messages: "Node address must start with wss:// (or ws:// for this computer)"; "This node is already in the list".
4. **This device.** `stats()`: records, authors, products, listings, reviews, conflicting histories (`forks`), invalid records, removed bodies (`redacted`).
5. **Build.** App version; wasm SHA-256 (full, grouped, copy); policy hash. "Compare these with the checksums on the release page. If they differ, this app is not the published release." Link to the release checksums.

States: no nodes ("No nodes configured. Add one to receive records."); all disconnected (offline banner); node errors (state "Disconnected" plus the last error in plain words, for example "Refused: unsupported protocol version").

### 8.8 Policy

Purpose: the rules that compute every view (7.1, views §2). Route: `#/policy`.

Content, in order:

1. **Header.** "Policy 7be0c2a1… (default)" or "(custom: 2 settings differ from the default)". Copy hash. The differing settings are listed with default and current values.
2. **Trusted attestors.** Table: attestor chip; what they are trusted for (checkboxes "Identity", "Shop domains", "Revocations" for `attest.identity`, `attest.domain`, `attest.revoke`); "Ignore records after #" (`not_after_seq`, empty = none); "Remove".
   - Add: field "Attestor key". Warning before adding: "Every key this attestor verifies will count as a separate verified person under your policy. Each extra identity attestor lets one person count once more (threat-model R3)."
3. **Moderation.**

   | Setting | Field | Control | Default | Effect sentence |
   |---|---|---|---|---|
   | Flags to hide an item | `flag_threshold` | number ≥ 1 | 3 | "Listings and profiles are hidden at this many counted flags." |
   | Flags to hide a review | `flag_thresholds.review` | number ≥ 1 | 5 | "Reviews are hidden at this many counted flags." |
   | Flags to hide a product record | `flag_thresholds.product` | number ≥ 1 | 5 | "Product records are hidden at this many counted flags." |
   | Count only verified flaggers | `flags_require_verified` | switch | on | "Off: flags from any key count. Creating keys costs nothing." |
   | Hidden listings to restrict a seller | `seller_hidden_listing_limit` | number ≥ 1 | 3 | — |
   | Distinct flaggers to restrict a seller | `seller_min_distinct_flaggers` | number ≥ 0 | 9 | — |

4. **Purchases.** "Verified purchase requires a verified reviewer" (`verified_purchase_requires_verified_buyer`, default on).
5. **Prices.** "Count only listings on verified shop domains" (`price_requires_domain_verified`, default on). Turning it off shows: "Listings on unverified domains will count in price summaries. A copy of a real shop with a low price can then appear as the lowest price."
6. **Time.** "Allowed clock difference" (`future_ts_tolerance_ms`, shown in minutes, default 10). "Views use this device's time, updated every minute."
7. **Actions.** "Apply" (shows the new hash first: "New policy hash: 91c4…"), "Undo last change", "Reset to default" (confirm, listing what changes), "Export policy (JSON)", "Import policy", "Compare with a node's policy" (fetches `/v1/policy`, shows a diff, and "Use this policy" applies it only on that explicit action, 10).

Validation messages (views §2.1): "`<Setting>` must be a whole number of 1 or more"; "`<Setting>` must be a whole number of 0 or more"; "Attestor key must be 64 characters, 0 to 9 and a to f"; "This attestor is already in the list"; "Select at least one thing to trust this attestor for"; "The file is not a valid Perch policy: `<field>` `<problem>`".

Defaults come from `web/default-policy.json` (10). The values above are the contract defaults (7.1).

### 8.9 Data

Purpose: export, import, and inspect signed records. Routes: `#/data`, `#/data/<id>`.

#### 8.9.1 Export and import

- "Download all records (JSONL)": `client.exportJsonl()`, file `perch-<YYYY-MM-DD>.jsonl`, with the record count and size in the button's description.
- Import: file or paste. Result from `client.ingest()`:

  | Field | Label | Sentence |
  |---|---|---|
  | `inserted` | Added | "New records, now on this device." |
  | `duplicate` | Already here | "Records this device already had." |
  | `gap` | Waiting | "Held until earlier records of the same log arrive." |
  | `fork` | Conflict proof | "Two different records at one position. Stored as proof; the author is marked." |
  | `rejected` | Rejected | "Failed the signature, hash, or format check. Not stored." |

  `errors` are listed with line number and message, first 100, then "Download all errors".

#### 8.9.2 Record inspector

Input: a record ID (from a link) or pasted record JSON.

Content, in order:

1. **Verdict.** From `client.verifyRecord(json)`: "Signature valid. The record ID matches its content." or the specific failure: "Signature does not match the author's key", "The body does not match body_hash", "The record ID in the link does not match this content".
2. **Header fields.** Author (chip), position (`seq`), previous record (link), stated time (local date and ISO 8601 UTC), kind, replaces (link), body hash with "Matches body" or "Body removed".
3. **Status in views.** From `client.recordStatus(id)`: "Used by views", or "Ignored: `<reason>`" (section 12.3); chain root and versions (links); `ignored` reason when set.
4. **Body.** Pretty-printed JSON rendered as text (never HTML). Toggle: "Exact signed line" shows the canonical line. Buttons: "Copy record", "Download record".
5. **References.** Links to target, listing, product, and replaced records.
6. **Verify it yourself.** The commands below, with the record's file name filled in.

```sh
# record.json holds one record line exactly as exported.
jq -jcS 'del(.sig, .body)' record.json > header.json   # canonical header
sha256sum header.json                                   # equals the record ID
jq -jcS '.body' record.json | sha256sum                 # equals body_hash (see note)
{ printf 'perch/record/v1\n'; cat header.json; } > signed.bin
{ printf '302a300506032b6570032100'; jq -r .author record.json; } | tr -d '\n' \
  | perl -pe 's/(..)/chr hex $1/ge' > author.der
jq -jr .sig record.json | perl -pe 's/(..)/chr hex $1/ge' > sig.bin
openssl pkeyutl -verify -pubin -keyform DER -inkey author.der -rawin \
  -in signed.bin -sigfile sig.bin                       # "Signature Verified Successfully"
```

Notes shown with the commands:

- Tested with jq 1.7 and OpenSSL 3.0.13 on a signed test record.
- Header keys are ASCII, so `jq -S` key order equals RFC 8785 order. For bodies, `jq` can differ from RFC 8785: it escapes U+007F, and it orders keys by code point, which differs from UTF-16 order for characters above U+FFFF. Use an RFC 8785 library for such bodies.
- The app uses strict Ed25519 verification (4.2). A record the app accepts also passes OpenSSL. OpenSSL may accept rare signatures that the app rejects.

## 9. Browser extension

Purpose: show Perch data for the product on a shop's page, without leaving the page and without revealing the product to anyone (10, 8.1).

### 9.1 Detection

- The content script reads schema.org `Product` markup: JSON-LD and microdata, properties `gtin`, `gtin8`, `gtin12`, `gtin13`, `gtin14` ([schema.org](https://schema.org/Product)). A `gtin` value that is a GS1 Digital Link URL is read for its GTIN.
- It keeps only GTINs with a valid check digit.
- Exactly 1 distinct GTIN: show the panel. 2 or more distinct GTINs (category pages, bundles): no panel; the toolbar button shows the count, and its popup lets the user pick one. 0: no panel; the toolbar button is grey with the tooltip "No product number found on this page."
- Detection makes no network request.

### 9.2 Panel placement

- Collapsed by default: a tab 40 px high at the bottom right, reading "Perch 4.2 (37)" (verified-identity mean and count) or "Perch: no rating". It covers at most 200 × 40 px of the page.
- Expanded on click or with the shortcut Alt+Shift+P (commands API). Width `min(360px, 100vw − 32px)`; height fits content up to `100vh − 32px`, then scrolls inside.
- Escape and the close button collapse it. "Hide on this site" removes it for that host until re-enabled in options.
- The expanded state persists across pages in `chrome.storage.local`.
- The panel never auto-expands, never animates for attention, and never covers the page while the user types in a page field.
- The panel is an extension page in an iframe, so the shop's page cannot read its content. The shop's page can detect that an iframe was added. Options offer "Toolbar button only", which leaves the page unchanged.

### 9.3 Panel content

In order:

1. **Product.** Name (`<bdi>`) and printed GTIN (8.2 rule). "Not this product?" links to the toolbar popup when the page has several GTINs.
2. **Rating.** "4.2 from 37 identity-verified reviewers." Then "Verified purchases: 4.4 from 12" and "All counted reviews: 3.9 from 58". Same rules as T6.
3. **Prices.** Per currency: "€89.00 to €129.00 from 5 listings on verified shop domains." Plus "2 listings not counted: shop domain not verified."
4. **This site.** The page's host (from the content script, never sent): "shop.example is verified as the shop of `<seller name>` by `<attestor>` until `<date>`." or "No seller in Perch has a verified domain for shop.example. Check the address before you pay." When a listing in the related set has this host: its price and availability.
5. **Moderation.** "2 reviews and 1 listing are hidden by moderation." (link to the product in the app)
6. **Sources.** "Checked 2 nodes: node-a.example, node-b.example. Policy 7be0c2a1." Disagreement: "Nodes returned different records" with each node's verified-identity count and price range, and "The numbers above combine the records from all nodes. A node with fewer records may be behind or leaving some out." Rejected records: "3 records failed verification and were ignored."
7. **Privacy line.** "Sent: the hash prefix 3fa9 to 2 nodes. Not sent: this page's address, the product, or cookies." The prefix shown is the one actually sent.
8. **Link.** "Open in Perch" opens `#/product/<key>` in the configured web app. The product key is in the fragment, which the browser does not send to the server.

The extension holds no key and cannot publish (10). "Write a review" opens the web app's review form for this product.

### 9.4 Panel states

| State | Collapsed tab | Expanded text |
|---|---|---|
| Checking | "Perch …" | "Checking Perch records for GTIN 4006381333931." |
| No records | "Perch: no data" | "No Perch records for GTIN 4006381333931 on the 2 nodes checked." Link: "Add this product" (web app). |
| No node answered | "Perch: offline" | "No Perch node answered. node-a.example: timed out." Link: "Choose nodes". |
| Some nodes failed | normal | Normal content plus "node-b.example did not answer (refused)." |
| Nodes disagree | normal + triangle icon | Section 9.3 item 6. |
| Not set up | none | The panel does not appear before the first-run page is completed (9.5). |

### 9.5 Options and first run

- On install, the options page opens. It states what leaves the browser, in a table:

  | Data | Sent to | When |
  |---|---|---|
  | First 4 hex characters of `sha256(product key)` | Each node in the list | Once per product page with a GTIN |

  "Nothing else leaves the browser: no page address, no full product key, no cookies, no Referer header."
- Node list: default `http://127.0.0.1:7700`. Adding a remote node asks the browser for permission for that host (optional host permissions), so the user approves each node.
- Prefix privacy, with numbers (threat-model R6): "A 4-character prefix splits products into 65,536 groups. On a node with 100,000 products, a group holds about 1.5 products, so the node can often tell which product you viewed. On a node with 1,000,000 products, a group holds about 15."
- Settings: node list; web app address; "Show Perch: On the page (default) / Toolbar button only"; "Hidden on these sites" list.

## 10. Visual direction and tokens

### 10.1 Direction

- Calm and information-dense. Neutral surfaces, one accent color for links and actions, status colors only for status.
- Text first. Icons always come with text. No decorative images, gradients, or illustrations.
- Numbers are the loudest elements: rating means and prices use the largest type on a card.
- Every screen works at 360 px wide and up to 72 rem (1,152 px) content width. No horizontal page scroll; wide tables become cards below 600 px.

### 10.2 Color tokens

Names follow `web/styles.css`. Values are the current app values except the 5 changed tokens marked ✱ (section 15). Contrast ratios are [WCAG 2.2](https://www.w3.org/TR/WCAG22/) relative-luminance ratios, computed for each pair.

| Token | Light | Dark | Use |
|---|---|---|---|
| `--page` | `#eef1f0` | `#131a1f` | Page background |
| `--surface` | `#ffffff` | `#1b252b` | Cards, panels, chart surface |
| `--surface-2` | `#f6f8f7` | `#212d34` | Table stripes, code blocks |
| `--ink` | `#18242b` | `#e6ecee` | Body text |
| `--ink-2` | `#46545c` | `#b4c0c5` | Secondary text |
| `--muted` | `#5f6c74` | `#93a0a6` | Hints, meta text |
| `--line` | `#d3d9dc` | `#33424a` | Decorative dividers only |
| `--line-strong` ✱ | `#76838a` | `#71838c` | Input borders, control outlines, unfilled stars |
| `--accent` | `#2f3aa3` | `#a6abff` | Links, primary buttons |
| `--accent-ink` | `#ffffff` | `#131a1f` | Text on `--accent` |
| `--accent-tint` | `#e9eafb` | `#262c55` | Selected rows, info notices |
| `--good` / `--good-tint` | `#17703e` / `#e3f3e8` | `#5cc58a` / `#183224` | Verified, valid |
| `--warn` / `--warn-tint` | `#8a5300` / `#fbf0d9` | `#e7b257` / `#33291a` | Domain not verified, disagreement |
| `--bad` / `--bad-tint` | `#a32b26` / `#fbe7e5` | `#f08a83` / `#3a2120` | Forked, revoked, restricted, failures |
| `--hidden-tint` | `#eceff1` | `#222c31` | Moderation notice background |
| `--focus` | `#2f3aa3` | `#a6abff` | Focus ring |
| `--star` | `#b06c00` | `#e7b257` | Filled stars |
| `--star-off` ✱ | `#76838a` | `#71838c` | Unfilled star outlines (same as `--line-strong`) |
| `--series-1` | `#2a78d6` | `#3987e5` | Chart series 1 |
| `--series-2` ✱ | `#c75a1c` | `#d95926` | Chart series 2 |
| `--series-3` ✱ | `#0f8a5f` | `#199e70` | Chart series 3 |
| `--grid` | `#e1e5e7` | `#2a363c` | Chart gridlines (decorative) |
| `--axis` ✱ | `#76838a` | `#71838c` | Chart axis lines and ticks |

`--series-4` is removed (section 10.6).

Contrast of every text and control pair (minimum: 4.5:1 for text, 3:1 for controls and chart marks, [WCAG 1.4.3 and 1.4.11](https://www.w3.org/TR/WCAG22/#contrast-minimum)):

| Text or mark | On | Light | Ratio | Dark | Ratio | Minimum |
|---|---|---|---|---|---|---|
| `--ink` | `--page` | `#18242b` / `#eef1f0` | 13.93 | `#e6ecee` / `#131a1f` | 14.72 | 4.5:1 |
| `--ink` | `--surface` | `#18242b` / `#ffffff` | 15.84 | `#e6ecee` / `#1b252b` | 13.07 | 4.5:1 |
| `--ink` | `--hidden-tint` | `#18242b` / `#eceff1` | 13.71 | `#e6ecee` / `#222c31` | 11.95 | 4.5:1 |
| `--ink-2` | `--surface` | `#46545c` / `#ffffff` | 7.83 | `#b4c0c5` / `#1b252b` | 8.38 | 4.5:1 |
| `--ink-2` | `--page` | `#46545c` / `#eef1f0` | 6.89 | `#b4c0c5` / `#131a1f` | 9.44 | 4.5:1 |
| `--muted` | `--surface` | `#5f6c74` / `#ffffff` | 5.41 | `#93a0a6` / `#1b252b` | 5.81 | 4.5:1 |
| `--muted` | `--page` | `#5f6c74` / `#eef1f0` | 4.76 | `#93a0a6` / `#131a1f` | 6.54 | 4.5:1 |
| `--muted` | `--surface-2` | `#5f6c74` / `#f6f8f7` | 5.07 | `#93a0a6` / `#212d34` | 5.25 | 4.5:1 |
| `--muted` | `--hidden-tint` | `#5f6c74` / `#eceff1` | 4.68 | `#93a0a6` / `#222c31` | 5.31 | 4.5:1 |
| `--muted` | `--accent-tint` | `#5f6c74` / `#e9eafb` | 4.54 | `#93a0a6` / `#262c55` | 4.97 | 4.5:1 |
| `--muted` | `--warn-tint` | `#5f6c74` / `#fbf0d9` | 4.78 | `#93a0a6` / `#33291a` | 5.31 | 4.5:1 |
| `--muted` | `--bad-tint` | `#5f6c74` / `#fbe7e5` | 4.55 | `#93a0a6` / `#3a2120` | 5.51 | 4.5:1 |
| `--muted` | `--good-tint` | `#5f6c74` / `#e3f3e8` | 4.70 | `#93a0a6` / `#183224` | 5.14 | 4.5:1 |
| `--accent` | `--surface` | `#2f3aa3` / `#ffffff` | 9.31 | `#a6abff` / `#1b252b` | 7.35 | 4.5:1 |
| `--accent` | `--page` | `#2f3aa3` / `#eef1f0` | 8.19 | `#a6abff` / `#131a1f` | 8.28 | 4.5:1 |
| `--accent` | `--accent-tint` | `#2f3aa3` / `#e9eafb` | 7.82 | `#a6abff` / `#262c55` | 6.28 | 4.5:1 |
| `--accent-ink` | `--accent` | `#ffffff` / `#2f3aa3` | 9.31 | `#131a1f` / `#a6abff` | 8.28 | 4.5:1 |
| `--good` | `--good-tint` | `#17703e` / `#e3f3e8` | 5.33 | `#5cc58a` / `#183224` | 6.45 | 4.5:1 |
| `--good` | `--surface` | `#17703e` / `#ffffff` | 6.13 | `#5cc58a` / `#1b252b` | 7.29 | 4.5:1 |
| `--warn` | `--warn-tint` | `#8a5300` / `#fbf0d9` | 5.60 | `#e7b257` / `#33291a` | 7.40 | 4.5:1 |
| `--warn` | `--surface` | `#8a5300` / `#ffffff` | 6.33 | `#e7b257` / `#1b252b` | 8.10 | 4.5:1 |
| `--bad` | `--bad-tint` | `#a32b26` / `#fbe7e5` | 6.03 | `#f08a83` / `#3a2120` | 6.11 | 4.5:1 |
| `--bad` | `--surface` | `#a32b26` / `#ffffff` | 7.17 | `#f08a83` / `#1b252b` | 6.44 | 4.5:1 |
| `--line-strong` ✱ | `--surface` | `#76838a` / `#ffffff` | 3.90 | `#71838c` / `#1b252b` | 3.96 | 3:1 |
| `--line-strong` ✱ | `--page` | `#76838a` / `#eef1f0` | 3.43 | `#71838c` / `#131a1f` | 4.46 | 3:1 |
| `--line-strong` ✱ | `--surface-2` | `#76838a` / `#f6f8f7` | 3.66 | `#71838c` / `#212d34` | 3.58 | 3:1 |
| `--line-strong` ✱ | `--hidden-tint` | `#76838a` / `#eceff1` | 3.38 | `#71838c` / `#222c31` | 3.62 | 3:1 |
| `--focus` | `--surface` | `#2f3aa3` / `#ffffff` | 9.31 | `#a6abff` / `#1b252b` | 7.35 | 3:1 |
| `--focus` | `--page` | `#2f3aa3` / `#eef1f0` | 8.19 | `#a6abff` / `#131a1f` | 8.28 | 3:1 |
| `--star` | `--surface` | `#b06c00` / `#ffffff` | 4.21 | `#e7b257` / `#1b252b` | 8.10 | 3:1 |
| `--series-1` | `--surface` | `#2a78d6` / `#ffffff` | 4.42 | `#3987e5` / `#1b252b` | 4.29 | 3:1 |
| `--series-2` ✱ | `--surface` | `#c75a1c` / `#ffffff` | 4.28 | `#d95926` / `#1b252b` | 4.02 | 3:1 |
| `--series-3` ✱ | `--surface` | `#0f8a5f` / `#ffffff` | 4.36 | `#199e70` / `#1b252b` | 4.58 | 3:1 |
| `--axis` ✱ | `--surface` | `#76838a` / `#ffffff` | 3.90 | `#71838c` / `#1b252b` | 3.96 | 3:1 |
| `--line` | `--surface` | `#d3d9dc` / `#ffffff` | 1.43 | `#33424a` / `#1b252b` | 1.50 | decorative only |
| `--grid` | `--surface` | `#e1e5e7` / `#ffffff` | 1.27 | `#2a363c` / `#1b252b` | 1.26 | decorative only |

Rules:

- Colors are defined once on `:root`, redefined under `@media (prefers-color-scheme: dark)` with `:root:not([data-theme='light'])`, and again under `:root[data-theme='dark']`, as `web/styles.css` already does.
- `--line` may not be the only boundary of a control. Inputs, checkboxes, radios, and buttons without fill use `--line-strong`.
- In `forced-colors: active`, use system colors (`CanvasText`, `LinkText`, `Highlight`) and keep icons and text labels; never rely on backgrounds for status.

### 10.3 Typography

- Font stack (body): `'Atkinson Hyperlegible Next', system-ui, -apple-system, 'Segoe UI', sans-serif`. Mono (keys, hashes, JSON): `'Atkinson Hyperlegible Mono', ui-monospace, 'SFMono-Regular', Menlo, Consolas, monospace`. Both are self-hosted (no requests to font services). The system stack follows as fallback.
- Scale (rem at 16 px root):

  | Token | Size | Line height | Weight | Use |
  |---|---|---|---|---|
  | `--text-xs` | 0.8125 rem (13 px) | 1.4 | 400 | Chart ticks only |
  | `--text-sm` | 0.875 rem (14 px) | 1.45 | 400 | Meta, hints, table cells on mobile |
  | `--text-md` | 1 rem (16 px) | 1.5 | 400 | Body, inputs |
  | `--text-lg` | 1.125 rem (18 px) | 1.4 | 600 | Card titles, `h3` |
  | `--text-xl` | 1.375 rem (22 px) | 1.3 | 650 | `h2` |
  | `--text-2xl` | 1.75 rem (28 px) | 1.25 | 700 | `h1`, rating means, prices on cards |
  | `--text-3xl` | 2.25 rem (36 px) | 1.15 | 700 | One hero number per screen (product rating or listing price) |

- Inputs use at least 16 px so mobile browsers do not zoom on focus.
- `font-variant-numeric: tabular-nums` in tables and chart ticks only.
- Text measure: at most 68 ch for prose (`--measure`).
- Text must survive 200% zoom and the WCAG 1.4.12 spacing overrides without loss.

### 10.4 Spacing, size, and shape

| Token | Value | Use |
|---|---|---|
| `--space-1` | 4 px | Icon to text |
| `--space-2` | 8 px | Inside chips and badges; between related lines |
| `--space-3` | 12 px | Between fields' label, hint, and input |
| `--space-4` | 16 px | Card padding on mobile; page gutter (`--gutter`) |
| `--space-5` | 24 px | Card padding on desktop; between fields |
| `--space-6` | 32 px | Between sections |
| `--space-7` | 48 px | Between screen header and content on desktop |
| `--radius` | 6 px | Controls, badges |
| `--radius-lg` | 10 px | Cards, panels |
| Target size | ≥ 24 × 24 px (WCAG 2.5.8); 44 × 44 px for primary actions and the extension tab | Buttons, links in lists, filter chips |
| Focus ring | 2 px solid `--focus`, 2 px offset | Every focusable element; never removed |

Breakpoints: below 600 px (single column, cards instead of tables, nav menu); 600 to 959 px (single column, tables allowed); 960 px and up (two columns on Product: main content and a 20 rem summary column).

### 10.5 Icons

- 16 px line icons, `currentColor`, `aria-hidden="true"`; the adjacent text carries meaning.
- Set: shield (identity), globe (domain), receipt (purchase), flag (moderation), fork (conflicting history), key (revoked), clock (retired), ban (restricted), triangle (warning), check (valid), image, copy, external link.

### 10.6 Charts

Color-vision checks below use simulated protanopia, deuteranopia, and tritanopia and report the smallest color difference between any 2 series as OKLab ΔE × 100.

- **Price history**: step line per listing (2 px), seller-stated points as 8 px circles, verified observations as 8 px diamonds, both with a 2 px `--surface` ring. One chart per currency; never two y-axes.
- **Series limit: 3.** Slots `--series-1` to `--series-3`. The 3 light-mode colors pass all-pairs checks for color-vision deficiency (worst pair ΔE 8.8, protan; OKLab × 100, target ≥ 8) and normal vision (worst ΔE 20.8, floor 15), and each has ≥ 4.28:1 contrast on `--surface`. The dark set: worst CVD ΔE 9.4, normal 20.9, contrast ≥ 4.02:1. The previous 4th slot (`#eda100`, yellow) failed: 2.17:1 contrast on white, and the orange–yellow pair is hard to separate.
- Colors follow the listing, not its rank: slot = position in the user's chosen set, kept when others are added or removed.
- Legend always for 2 or more series; direct label of the latest price at the line's end for 1 series.
- Hover and keyboard: a vertical crosshair snaps to dates; the readout lists every series at that date, value first. Arrow keys move it when the chart has focus.
- Every chart has "Show as table" with every point, its source, and its record link. Tooltips never hold the only copy of a value.
- **Rating histogram**: 5 horizontal bars, one series color (`--series-1`), 16 px thick, 4 px rounded end, count label at the bar end in `--ink-2`. Rows are buttons (filter). Empty rows show a 0 label and no bar.
- Axis and tick text uses `--muted`; gridlines `--grid`; axis lines `--axis`.

### 10.7 Motion

- Transitions ≤ 150 ms, opacity and color only. None under `prefers-reduced-motion: reduce`.
- Re-renders from sync keep scroll and focus. Changed numbers are not animated.

## 11. Accessibility requirements

Target: [WCAG 2.2](https://www.w3.org/TR/WCAG22/) level AA for the web app, the extension panel, and the options page. Phase 2 tests each row with Playwright and axe-core in headless Chromium.

| Requirement | Rule | WCAG |
|---|---|---|
| Structure | One `<h1>` per screen; headings in order; landmarks `header`, `nav`, `main`, `footer`; skip link to `#main` | 1.3.1, 2.4.1 |
| Keyboard | Every action works with the keyboard; tab order follows reading order; no keyboard traps; Escape closes disclosures that overlay content | 2.1.1, 2.1.2, 2.4.3 |
| Focus visible | 2 px ring, 2 px offset, ≥ 3:1 against both neighbors; sticky header never covers the focused element (`scroll-padding-top` = header height) | 2.4.7, 2.4.11 |
| Focus on change | Route change: focus `<h1>`. Form error: focus the error summary. Expanding a notice: focus stays on its button | 2.4.3, 3.3.1 |
| Names | Every control has a visible label that starts its accessible name; icon-only buttons have `aria-label` ("Copy key c6488cb3") | 2.5.3, 4.1.2 |
| Status messages | Live region (`aria-live="polite"`) for copy results, publish results, sync progress; never for every re-render | 4.1.3 |
| Color | No information by color alone; status has icon and text (6.7); links in text are underlined | 1.4.1 |
| Contrast | Section 10.2 table | 1.4.3, 1.4.11 |
| Reflow and zoom | 320 CSS px wide at 400% zoom without loss; no horizontal scroll except data tables and code blocks inside their own scroll area | 1.4.10 |
| Text spacing | Survives line height 1.5, paragraph spacing 2×, letter spacing 0.12 em, word spacing 0.16 em | 1.4.12 |
| Targets | ≥ 24 × 24 px or spaced (10.4) | 2.5.8 |
| Dragging | Chart crosshair also works with arrow keys and tap; nothing requires dragging | 2.5.7 |
| Forms | Labels, hints via `aria-describedby`, errors via `aria-describedby` and `aria-invalid`, error summary with links; input kept on error | 3.3.1, 3.3.2, 3.3.3 |
| Paste | Keys, backups, and receipts can be pasted; no "type it again" checks | 3.3.7, 3.3.8 |
| Language and direction | `lang` on `<html>`; record text with `dir="auto"` or `<bdi>` (10) | 3.1.1, 1.3.2 |
| Tables | `<th scope>`; captions; card layout keeps label–value pairs as `<dl>` | 1.3.1 |
| Tabs and disclosures | `<details>` for disclosures; ARIA tabs pattern for Author records | 4.1.2 |
| Charts | `role="img"` with a summary label ("Price history in EUR, 3 listings, from €89.00 to €129.00"), plus the table view | 1.1.1 |
| Motion | `prefers-reduced-motion` respected | 2.3.3 (AAA, adopted) |
| Consistent help | Footer links ("How Perch works", "Data", "Policy") in the same order on every screen | 3.2.6 |

## 12. Copy guidelines

### 12.1 Rules

1. Plain words. Use the glossary terms in section 13; avoid protocol words in the main UI ("record" is allowed; "envelope", "canonical", "seq" only in the inspector).
2. One point per sentence. Sentences under 20 words where possible.
3. Say who did it: "5 verified people flagged this", not "This was flagged".
4. Numbers as numerals, localized: "3 reviews", "1,284 records".
5. State facts, not judgments: "Domain not verified", not "Unsafe shop". The user decides.
6. Claims carry their source: "Price stated on 3 Oct 2026 by the seller".
7. Sentence case for everything: headings, buttons, labels.
8. Buttons start with a verb and name the object: "Download backup", "Sign and publish", "Show flags (7)".
9. No "please", "sorry", "oops", "unfortunately", exclamation marks, or humor ([GOV.UK](https://design-system.service.gov.uk/components/error-message/)).
10. Never "trusted", "safe", "genuine", "official", "best", "top", "recommended" (T4).
11. Record text is quoted only by layout (its own block or `<bdi>`), never inside app sentences without isolation.

### 12.2 Error and warning messages

- Say what is wrong and how to fix it, with the field's own label: "Enter the brand".
- Empty field: an instruction ("Enter …", "Select …"). Broken rule: a description ("… must be 300 characters or fewer").
- Never "invalid", "error occurred", "forbidden", "illegal input" (except the flag reason "Illegal"), or codes alone. Technical detail goes after the plain sentence: "Perch could not connect to node-a.example. The node refused the connection (unsupported protocol version)."
- Warnings explain the consequence: "This listing will not count in price summaries until …".
- The same message text appears in the error summary and next to the field.

### 12.3 Label tables

Availability (`listing.availability`): `in_stock` "In stock"; `out_of_stock` "Out of stock"; `preorder` "Preorder"; `discontinued` "Discontinued".

Condition: `new` "New"; `used` "Used"; `refurbished` "Refurbished".

Roles (`profile.role`): `seller` "Seller"; `buyer` "Buyer"; `tester` "Tester"; `crawler` "Price crawler"; `attestor` "Attestor"; `node` "Node operator"; `other` "Other". Roles are chosen by the author; they are not verified.

Not counted in ratings (`ReviewView.excluded_reason`):

| Value | Text |
|---|---|
| `retracted` | withdrawn by its author |
| `unavailable` | content removed by a node |
| `seller` | written by a seller of this product |
| `forked` | author's key has a conflicting history |
| `revoked` | written after the key was reported stolen |
| `superseded` | the same person has a newer review of this product |
| `hidden` | (not used: hidden reviews count) |

Listing not in price summary (views §8.3, first failing rule): "hidden by moderation"; "not in stock"; "seller is restricted"; "shop domain not verified".

Ignored by views (`RecordStatus.reason` and `ignored`):

| Code | Text |
|---|---|
| `schema:<field>` | "The `<field>` field breaks the format rules." |
| `replaces:not_found` | "It replaces a record that is not on this device." |
| `replaces:author` | "It replaces a record by another author." |
| `replaces:seq` | "It replaces a record that is not older." |
| `replaces:kind` | "It replaces a record of another kind." |
| `replaces:not_replaceable` | "This kind of record cannot be replaced." |
| `fixed:<field>` | "It changes `<field>`, which cannot change after publishing." |
| `retract:author` | "It withdraws another author's record." |
| `retract:seq` | "It withdraws a record that is not older." |
| `retract:target_kind` | "This kind of record cannot be withdrawn." |
| `appeal:author` | "It appeals a record by another author." |
| `ref:listing_kind` | "It refers to a listing that is not a listing." |
| `unavailable` | "Its content was removed by a node." |
| `unknown_kind` | "This version of Perch does not know this kind of record." |
| `future` | "Its stated time is in the future." |
| `forked` | "It is at or after the author's conflicting history." |
| `revoked` | "It was signed after the key was reported stolen." |
| `retired` | "It was signed after the author retired the key." |
| `retracted` | "It is a new version of a withdrawn record." |
| `not_after_seq` | "Your policy ignores this attestor's records after #`<n>`." |

## 13. User-facing glossary

Shown at `#/help/glossary` and linked from disclosures. Plain definitions; contract terms in parentheses.

| Term | Definition |
|---|---|
| Record | One signed statement, such as a review, a listing, or a flag. Records never change. A new version replaces an old one, and both stay in the data. |
| Author | Whoever signed a record. An author is a key. One person can have several keys. |
| Key | Your identity in Perch. The public key (64 characters) is shown to everyone. The secret key signs your records; keep it private and backed up. |
| Signed | Made with the author's secret key. Anyone can check that the record came from that key and was not changed. |
| Identity verified | An attestor in your policy checked who owns the key and says so until a date. |
| Attestor | An organization that checks identities or shop domains and publishes the result. Your policy lists the attestors you accept. |
| Verified purchase | The seller signed a receipt for the reviewer's key, and the seller's identity is verified. Under the default policy, the reviewer's identity is verified too. |
| Receipt | A signed note from a seller that a key bought from a listing. It stays private until the buyer adds it to a review. |
| Shop domain verified | An attestor confirmed that the seller controls the web address the listing links to. |
| Listing | A seller's offer: price, availability, condition, and a link to the shop. |
| Price observation | A price someone saw on a shop's page, with the date. Only observations by verified people are used. |
| Flag | A public report that a record is a problem, with a reason. |
| Hidden | Collapsed because enough verified people flagged it under your policy. Hidden items stay in the data and you can open them. Hidden reviews still count in ratings. |
| Appeal | The author's public response to flags. It does not unhide anything by itself. |
| Withdrawn | The author took the record back. It stays in the data, marked withdrawn, and no longer counts (retract). |
| Removed by a node | A node operator removed the content of a record, for example by court order. The record's signature and date remain (redaction). |
| Conflicting history | The key signed two different records at the same position. Perch ignores its records from that point (fork). |
| Revoked | An attestor reported the key as stolen. Records after a stated point are ignored. |
| Retired | The owner ended the key. Later records are ignored. |
| Restricted seller | A seller with several hidden listings, flagged by many distinct verified people, or with a key that has a conflicting history. All their listings are marked. |
| Node | A computer that stores and passes on records. Anyone can run one. Nodes are not trusted for numbers; your device computes them. |
| Sync | Exchanging records with nodes until both sides have the same data. |
| Policy | Your settings for computing ratings, prices, and moderation: which attestors you accept and the thresholds. |
| Policy hash | A short code that identifies a policy exactly. Two people with the same records and the same policy hash see the same numbers. |
| Mean | The sum of ratings divided by their count. |
| Median | The middle price when prices are sorted; with an even count, the lower of the two middle prices. |
| GTIN | The number under a product's barcode (also called UPC or EAN). |
| MPN | The manufacturer's part number. |

## 14. Contract dependencies and open questions

### 14.1 Fields this spec depends on

All are in contract version 3 (7.8, 8.2).

| Spec item | Contract field or method |
|---|---|
| Purchase badge reasons (6.3) | `ReviewView.purchase` |
| Not-counted labels | `ReviewView.counted`, `excluded_reason`; `RatingSummary.excluded` |
| Moderation notice (6.5) | `Moderation`, `ListingView` and `ReviewView` `status`, `flags`, `appeal`, `redaction`; `ProductView.moderation`; `AuthorView.profile`; `client.flags()` → `FlagView` |
| Redaction notice (6.6) | `Redaction` |
| Business name (6.1) | `attest.identity.legal_name` → `AuthorView.verified.legal_name` |
| Author status (8.4) | `AuthorView.status`, `fork_seq`, `fork_evidence`, `revoked`, `retired`, `restriction` |
| Receipt share (8.3) | `AuthorView.seller_reviews` |
| Moderated search hits (8.1) | `SearchHit.moderation_status`; non-visible hits after visible hits |
| Ships to, location | `ListingView.ships_to`, `location` |
| Author records (8.4) | `client.authorRecords()` → `ChainSummary`; `client.authorLog()` |
| Inspector (8.9.2) | `client.verifyRecord()`, `client.recordStatus()` → `RecordStatus` |
| Publish gate (8.5.1) | `client.publishStatus()` with `local_head` and `nodes` |
| Money (6.9) | `client.currencies()` → `Currency` |
| Search sort and currency (8.1) | `client.search(q, { sort, currency })` |
| Receipts (8.5.4, 8.5.8) | `client.issueReceipt()`, `client.verifyReceipt()` |
| Build facts (T8) | `client.buildInfo()`, `client.policyHash()` |
| Extension (9) | `PerchClient.create({ sparse: true })`, `/v1/lookup/{prefix}` |

### 14.2 Open questions

| # | Question | Owner | Effect on UI |
|---|---|---|---|
| Q1 | `web/default-policy.json` has `flag_thresholds: { "review": 3 }`; contract 7.1 and views §2 give `{ "review": 5, "product": 5 }`. Which is the release default? | main | Policy defaults table (8.8); hidden counts in tests |
| Q2 | The release checksum page URL for the footer comparison (T8, 8.7). | main | Footer link |
| Q3 | Bootstrap node list for release builds (threat-model R10). | dist-systems | First-run Network state |
| Q4 | Should the extension's in-page panel be off by default ("Toolbar button only"), since the shop's page can detect an added iframe? This spec keeps it on, collapsed. | security | Section 9.2 |

## 15. Changes for the app

The app already implements most screens. These items differ from what is in `web/` and `extension/` today. Each names the file where the change likely lands.

| # | Change | Reason | File |
|---|---|---|---|
| C1 | Set `--line-strong` and `--axis` to `#76838a` (light) / `#71838c` (dark); set `--star-off` to the same values. | Current `#b4bec3` is 1.89:1 on white and `#4a5b64` is 2.21:1 on dark surface; controls and unfilled stars need 3:1 (WCAG 1.4.11). | `web/styles.css` |
| C2 | Set `--series-2` light to `#c75a1c` and `--series-3` light to `#0f8a5f`; remove `--series-4`; limit charts to 3 series with a "Choose listings" control (8.2). | Current light `#1baf7a` (2.82:1) and `#eda100` (2.17:1) fail 3:1; 4 slots fail the all-pairs color-vision check. | `web/styles.css`, `web/src/chart.js` (`SLOTS`) |
| C3 | Badges show the full sentence (6.2) with attestor and expiry, at least in the accessible description and on Author. "Domain x" becomes "Shop domain verified: x". | T4: no badge without who and until when. | `web/src/components.js`, `screens/author.js` |
| C4 | Never disable "Sign and publish" or "Sign receipt"; show the gate reason in the error summary on press (8.5.1). | Disabled buttons cannot be focused and give no reason. | `screens/publish.js` |
| C5 | Search: group hits with `moderation_status` ≠ `visible` after the visible ones in a collapsed notice (8.1). | T3; contract change K. | `screens/search.js` |
| C6 | Search: render `why` as sentences with a "Show raw" toggle (8.1). | Plain language; DSA Article 27 explanation. | `screens/search.js` |
| C7 | Listing and Author: show `seller_reviews` receipt share (8.3 item 7). | Threat-model R8; contract change L. | `screens/listing.js`, `screens/author.js` |
| C8 | Rating histogram rows become buttons that set a star filter; add the star radio group next to the source filter (8.2 item 10). | Baymard: 90% of users who wanted specific ratings clicked the bars; hidden filters were used by 7%. | `screens/product.js` |
| C9 | Product images with `sha256`: fetch, check hash, show from `blob:`; label "Hash not checked" when the fetch is blocked (8.2 item 4). | T1: the hash is a claim the app can check. | `web/src/components.js` |
| C10 | Show the printed GTIN form (8.2 item 1) instead of the 14-digit key. | Users compare with the barcode. | `web/src/format.js` (`keyLabel`) |
| C11 | Below 600 px, collapse the main nav into a "Menu" disclosure button (5.2). | 6 nav items plus search do not fit 360 px without wrapping. Verify in Phase 2. | `web/index.html`, `web/styles.css` |
| C12 | Extension: start collapsed as a 40 px tab; expand on click or Alt+Shift+P; Escape collapses (9.2). | The expanded 360 px panel at the maximum z-index can cover shop controls. | `extension/content.js`, `manifest.json` (`commands`) |
| C13 | Extension: show a panel only when the page has exactly 1 distinct GTIN (9.1). | `findGtin()` takes the first of several, which can show the wrong product on category pages. | `extension/content.js` |
| C14 | Extension: add "This site" (9.3 item 4) and the privacy line with the prefix actually sent (item 7). | The most useful fact on a shop's page is whether the shop's domain is verified; privacy is visible, not only documented. | `extension/panel.js` |
| C15 | Extension options: prefix-privacy numbers and "Toolbar button only" (9.5). | Threat-model R6; Q4. | `extension/options.html`, `options.js` |
| C16 | Footer and Network: add the release-checksum comparison link (T8, 8.7). | Q2. | `web/src/app.js`, `screens/network.js` |

Choices in the app that this spec adopts unchanged: the color token names and most values; the self-hosted Atkinson Hyperlegible fonts (they separate `0`/`O` and `1`/`l` in keys); the hash router; `textContent`-only rendering with `dir="auto"` and `<bdi>`; click-to-load images; step-line price charts with circle and diamond markers; the fingerprint glyph (as a decorative aid).

## Appendix A: terms and acronyms

| Term | Meaning |
|---|---|
| a11y | Accessibility. |
| ARIA | Accessible Rich Internet Applications, the W3C attributes for roles and states. |
| axe-core | An open-source accessibility test engine. |
| CORS | Cross-Origin Resource Sharing; browser rules for reading responses from other sites. |
| CVD | Color-vision deficiency (protanopia, deuteranopia, tritanopia). |
| DSA | EU Digital Services Act, Regulation (EU) 2022/2065. |
| ΔE (OKLab × 100) | Color difference in the OKLab color space, scaled by 100. |
| EAN, UPC | Barcode numbering systems; both are GTINs. |
| FTC | US Federal Trade Commission. |
| GS1 Digital Link | A URL form of GS1 identifiers such as GTIN. |
| GTIN | Global Trade Item Number. |
| ICU | International Components for Unicode; the locale data behind `Intl` in browsers and Node. |
| IDNA, punycode | Rules and encoding that turn non-ASCII host names into ASCII (`xn--…`). |
| `Intl` | The JavaScript internationalization API. |
| ISO 3166-1 alpha-2 | Two-letter country codes. |
| ISO 4217 | Currency codes and their minor units. |
| JSON-LD | JSON for Linked Data; how shops embed schema.org data. |
| JSONL | JSON Lines: one JSON value per line. |
| Minor unit | The smallest currency unit (cent for EUR); its number of decimal places comes from ISO 4217. |
| MPN | Manufacturer Part Number. |
| QR | Quick Response two-dimensional barcode. |
| R6, R8, R10, R12 | Residual risks in [`threat-model.md`](threat-model.md) section 7. |
| T1–T8 | Trust UI rules in section 4 of this document. |
| UTF-16 | A Unicode encoding; RFC 8785 sorts keys by its code units. |
| WCAG | Web Content Accessibility Guidelines. |
