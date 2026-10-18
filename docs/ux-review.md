# Perch UX and accessibility review

Status: Phase 2 review of the running web app and extension, contract version 3 (2026-10-18). Owner: ux. Spec under test: [`docs/ux.md`](ux.md).

## 1. Method

| Item | Value |
|---|---|
| Network | `tests/live/run.sh --no-build`: 3 nodes on 127.0.0.1:7701–7703, small demo dataset (321 records, 72 authors, 12 products) |
| App | `http://127.0.0.1:7701/`, real wasm client (SHA-256 `7445edceb859ccaf…`, matches `web/pkg/SHA256SUMS`) |
| Browser | Playwright, headless Chromium 141 |
| Viewports | 1280 × 800 and 360 × 800, each in light and dark color scheme |
| Screens | 18 routes: search, 4 products, 4 listings, 3 authors, publish, keys, network, policy, data, record inspector |
| Accessibility | [axe-core](https://github.com/dequelabs/axe-core) 4.10.2, tags `wcag2a`, `wcag2aa`, `wcag21aa`, `wcag22aa`, `best-practice`, on 12 routes × 4 configurations, with every `<details>` opened |
| Keyboard | 30 Tab presses per viewport on a product page; menu button with Enter, Tab, and Escape; focus after route change |
| Flows | Create key → back up → publish listing (with errors) → issue receipt → second browser writes a review with the receipt; change the review flag threshold in Policy |
| Extension | `tests/browser/extension.mjs` (real mode): 29 of 29 checks pass; screenshots under `shots/ext/` |

Screenshots are in `/tmp/claude-0/-home-claude/8a83982e-4361-5e73-82ac-bed8a7f25fbf/scratchpad/ux/review/shots/`. Paths below are relative to that folder. Full-page shots are named `<screen>-<width>-<scheme>.png`.

What works, measured:

- No horizontal scroll on any of the 18 routes at 360 px.
- No console or page errors in 72 screen loads.
- Every focusable element tested shows a 3 px focus ring in `--focus`. Focus moves to the `<h1>` after each route change. The skip link is the first stop.
- The 360 px menu opens with Enter, closes with Escape, and returns focus to its button.
- The color tokens from ux.md section 10.2 are in `web/styles.css`. axe reports no contrast failure outside the opacity issue in M11.
- Changing "Flags to hide a review" from 5 to 10 in Policy un-hides Ivan's review on the Cobalt H-300 page and changes the policy hash shown on Policy and in the footer.

## 2. Findings

Severity: **blocker** = defeats a trust guarantee or harms users; fix before release. **Major** = breaks a spec rule or WCAG 2.2 AA, or blocks a persona task. **Minor** = friction or copy.

### Blocker

#### B1. Listings from unverified shop domains lead "Where to buy" and fill the default price chart

- Screen: Product (Alder Kettle 1.7 L, `#/product/gtin:02000090000018`). Viewports: all.
- Screenshots: `crop-kettle-1280-a.png`, `crop-kettle-1280-b.png`, `crop-kettle-360-b.png`.
- Evidence:
  - The first 3 of 6 listings are from sellers with unverified shop domains: €9.99 (Tomorrow Deals), €19.99 ("G00dshop Deals", title "Alder Kettle 1.7 L - 60% OFF TODAY", host `goodshop-deals.example`), and €39.99.
  - The domain-verified Goodshop listing (€47.99, `www.goodshop.example`) is 4th. The price card above says €47.99, so the list contradicts the summary.
  - The price chart's default 3 series are the same 3 unverified listings. On Cobalt H-300 the default includes the hidden Quick Gadgets listing.
- Rule broken: D14 ("stops phishing listings from leading price rankings"); ux.md 8.2 items 7 and 8 (default chart = first 3 price-feeding listings); T5.
- Cause: views §8.4 sorts listings by status, then currency, then amount. Whether a listing feeds prices plays no part, and `ListingView` has no field that says so.
- Fix:
  1. `crates/perch-views` and `docs/views.md` §8.4: add `feeds_prices: bool` and `price_excluded_reason: string|null` to `ListingView` (ARCHITECTURE 7.8). Sort feeding listings first. Owner: analyst.
  2. `web/src/screens/product.js`: show 2 groups in view order: "In the price summary (2)", then "Other listings (4): shop domain not verified, withdrawn, hidden, or not for sale", collapsed at 360 px. Rename the heading "Where to buy" to "Listings". Perch does not sell.
  3. `web/src/chart.js` (`seriesFor`): default to the first 3 listings with `feeds_prices`. Never include hidden or withdrawn listings unless the user picks them.

### Major

#### M1. Hidden listings show their title, price, and shop link

- Screen: Product (Cobalt H-300) and Listing (`#/listing/f3b8f17b…`). Viewports: all.
- Screenshots: `crop-cobalt-hidden-listing-1280.png`, `crop-cobalt-1280-dark.png`, `crop-listing-hidden-360.png`.
- Evidence: the Quick Gadgets listing was flagged as counterfeit and fraud by 3 verified people, and is hidden. It still shows "Cobalt H300 headphones ORIGINAL", €89.99 (struck through), and a working "Seller page" link. The moderation notice sits below the price.
- Rule broken: T3 ("its body is collapsed behind a moderation notice"); ux.md 6.5.
- Fix: `web/src/screens/product.js` and `web/src/screens/listing.js`. For `status` = `hidden`, keep the seller chip and status line. Put the notice first. Put title, price, and shop link behind "Show listing", the same pattern reviews already use ("Show the hidden review").

#### M2. No "How this was computed" disclosure with policy values

- Screen: Product rating and price cards; Author restriction. Viewports: all.
- Screenshots: `crop-kettle-1280-a.png`.
- Evidence: the product page has 11 `<details>` elements, none for an aggregate. The cards explain exclusions inline, which is good. They never name the policy fields used, the policy hash, or the time used (`policy.now`), so a user cannot reproduce or compare the number.
- Rule broken: T2; ux.md 6.4.
- Fix: add `computedDisclosure({ result, exclusions, policyFields, rule, policy })` to `web/src/components.js`. Use it after the rating mean (fields: `trusted_attestors` count, `verified_purchase_requires_verified_buyer`), after each price (`price_requires_domain_verified`), and after restriction on Author (`seller_hidden_listing_limit`, `seller_min_distinct_flaggers`). Each shows the short policy hash and `policy.now`.

#### M3. Global status banners are missing; the nav shows "1/1" in green with one node

- Screen: all. Viewports: all.
- Screenshots: `search-360-light.png`, `explore-search-1280.png`, `policy-changed-1280.png`.
- Evidence:
  - The default app connects only to the node that served it. No banner says "Connected to 1 node"; the warning exists only on Network.
  - The nav badge reads "1/1" in green. At 360 px the Menu button reads "Menu 1/1", with no explanation on screen.
  - After a policy change, no banner appears, and the footer shows the new hash without "custom".
  - `web/src/app.js` renders only mock and boot-error banners.
- Rule broken: T8; ux.md section 7 (one node, nodes disagree, policy differs, offline); threat-model R10.
- Fix:
  - `web/src/app.js`: render the 4 banners of ux.md section 7 into `#banner`, as `role="status"` regions.
  - In `updateNavStatus()`, replace "1/1" with a text label: "1 node" with the warning state when fewer than 2 nodes are live, "3 nodes" when all agree, and "Nodes disagree" otherwise. Remove the count from the Menu button and keep a status dot with visible text in the menu.
  - Footer: "Policy 159e29f0… (default)" or "(custom)".

#### M4. "matches SHA256SUMS" in green overstates what was checked

- Screen: footer on every screen; Network; Data. Viewports: all.
- Screenshots: `explore-search-1280.png`.
- Evidence: the footer shows "Wasm SHA-256 7445edceb859ccaf matches SHA256SUMS" in `--good` color. The checksum file comes from the same node as the app, so an altered app would also match. Network says so in one sentence; the footer does not. `web/config.json` has `"release_page_url": ""`, so no independent comparison is offered.
- Rule broken: T4 (one precise meaning per badge); T8; ARCHITECTURE 2.1 ("Node: not trusted for served app code").
- Fix: `web/src/app.js` `checksumNote()`: neutral color, and the text "matches the checksum file served by this node". Set `release_page_url` in `web/config.json` and show "Compare with the release page" next to the hash on every screen. Owner for the URL: main (ux.md Q2).

#### M5. Appeals and flag details are not shown on visible listings

- Screen: Listing (Sound Corner, `#/listing/223a4615…`) and its Product card. Viewport: all.
- Screenshot: `crop-listing-appeal-1280.png`.
- Evidence: the API returns `appeal.note` "These are genuine EU units. Invoice from Cobalt available on request." The page shows only "1 flag". It shows no flag reason or flagger, and the appeal text appears nowhere in the app.
- Rule broken: T3 (appeal status shown); ux.md 6.5 and 8.3 item 6 (flags list and appeal whenever `flags.count` > 0); moderation.md 5.1 items 3 and 4.
- Fix: `web/src/screens/listing.js` and `product.js`: when `flags.count` > 0 or `appeal` is set, show the counts line, a "Show flags" disclosure with `client.flags(id)`, and the appeal note with date (`<bdi>`, `dir="auto"`). Same for reviews with `appeal`.

#### M6. Badge sentences for identity, business, retired, and conflicting history are only in `title`

- Screen: every author chip. Viewports: all, worst on touch.
- Screenshot: `crop-kettle-1280-a.png`.
- Evidence: in the 18 distinct badges on the kettle page, "Identity verified", "Business verified", "Retired", and "Conflicting history" carry the attestor, expiry, and meaning only in a `title` attribute. "Restricted seller" and the purchase badges have no sentence at all. Touch users and keyboard users cannot open `title` text, and screen readers do not read it reliably. Domain badges already include a visually hidden sentence.
- Rule broken: T4; ux.md 6.2; WCAG 1.3.1.
- Fix: `web/src/components.js` `badge()`: always append the sentence as visually hidden text, as domain badges already do. Make the badge a button that toggles the sentence visibly (a disclosure), so touch users can read "verified by Demo Identity Verifier A until Dec 31, 2027".

#### M7. Restriction caused by a forked key is explained as a flag count of zero

- Screen: Author (Mallory, `#/author/6d706142…`); review chip on Alder Kettle. Viewports: all.
- Screenshot: `crop-author-mallory-1280.png`.
- Evidence: Mallory is a buyer with no listings and a forked key. The page says "Restricted seller: 0 of this seller's listings are hidden, flagged by 0 verified people. Policy limits: 3 listings and 9 people." The numbers are below both limits, so the sentence contradicts the badge. Her review chip on the kettle page also shows "Restricted seller".
- Rule broken: T2, T4; views §6 rule 2 (restriction from an excluded-forked key).
- Fix:
  - Contract: add `restriction.reason: "flags"|"fork"` to `AuthorView` (ARCHITECTURE 7.8, views §15.1). Owner: analyst.
  - `web/src/screens/author.js` and `components.js`: for `fork`, show "Restricted: this person has a key with a conflicting history." Use the label "Restricted" for authors without listings, and "Restricted seller" only for sellers.

#### M8. Record inspector says "Body: None" for a record that has a body, and omits view status

- Screen: Data record inspector (`#/data/d399af62…`). Viewports: all.
- Screenshot: `crop-record-1280.png`.
- Evidence:
  - The field list shows "Body: None", while the signed JSON below contains a body with text and a receipt. Cause: `web/src/screens/data.js` line 176 passes `null` when the body is present.
  - The record shown is version 1 of Carol's review. The current version is "Switch broke". The inspector does not say that the record was replaced, and does not call `client.recordStatus()`.
  - The "verify it yourself" commands of ux.md 8.9.2 are missing.
- Rule broken: ux.md 8.9.2 items 2, 3, and 6; T1.
- Fix: `web/src/screens/data.js`:
  - Show "Present (hash matches)" for the body.
  - Add a "Status in views" row from `recordStatus` (`semantic_valid`, `reason`, `ignored`, and `versions` with "Replaced by" links), using the 12.3 labels.
  - Add the jq and OpenSSL block from ux.md 8.9.2.

#### M9. Form errors appear one at a time, below the button, and are not tied to fields

- Screen: Publish, every form. Viewports: all.
- Screenshots: `crop-listing-errors-1280.png`, `flow-listing-errors-1280.png`.
- Evidence:
  - Submitting an empty listing form shows only "Check the form. Choose a product." below the submit button.
  - The price error (`12.345` in EUR) hides the URL error (`http://…`); the user needed 3 submits to see all 3 problems.
  - The focused field has no `aria-invalid` and no message linked through `aria-describedby`.
- Rule broken: ux.md 8.5.2 item 3; [GOV.UK error message](https://design-system.service.gov.uk/components/error-message/); WCAG 3.3.1 and 3.3.3.
- Fix: `web/src/screens/publish.js` `publishForm()`:
  - Collect all errors from `build()` instead of throwing on the first.
  - Render an error summary at the top of the form (links to fields) and focus it.
  - Put each message above its input, with `aria-invalid="true"` and the message ID in `aria-describedby`.

#### M10. Publishing signs immediately; there is no review step

- Screen: Publish. Viewports: all.
- Screenshot: `flow-listing-published-1280.png`.
- Evidence: "Sign and publish" signs and sends the record on the first valid click. The only preview is a collapsed "Preview the record body". The lede says records are permanent.
- Rule broken: T7 ("Publishing always shows what becomes public … before the user signs"); ux.md 8.5.2 item 4.
- Fix: `web/src/screens/publish.js`: the first button reads "Review". It shows "This will be public" (fields, author key, stated time) with "Edit" and "Sign and publish".

#### M11. Withdrawn and not-counted items fail text contrast through opacity

- Screen: Product reviews (kettle, Cobalt), flag lists. Viewports: all; light and dark.
- Screenshot: `crop-kettle-1280-c.png` (Community Member 11, withdrawn).
- Evidence: axe `color-contrast`, 16 to 26 nodes per page. Examples: `#737f85` on `#eef1f0` = 3.62:1; `#7f8990` on `#f0f2f4` = 3.18:1; dark `#818d93` on `#20292e` = 4.34:1. Cause: `.review.not-counted { opacity: 0.86 }` and `.flags li.not-counted { opacity: 0.8 }` in `web/styles.css` (lines 467, 615).
- Rule broken: WCAG 1.4.3 (4.5:1); ux.md 10.2.
- Fix: `web/styles.css`: remove both opacity rules. Mark these items with the existing dashed border and the "Not counted …" text, which already carry the meaning.

#### M12. Star ratings in reviews differ only by hue, with no visible number

- Screen: Product reviews. Viewports: all.
- Screenshot: `crop-kettle-1280-c.png`.
- Evidence: filled stars (`--star` `#b06c00`) and unfilled stars (`--star-off` `#76838a`) use the same glyph and differ by 1.08:1 in luminance in light mode and 2.05:1 in dark mode. The rating number exists only in `aria-label`. ux.md 10.2 set `--star-off` without requiring a shape difference; that spec error is corrected in ux.md.
- Rule broken: WCAG 1.4.1 (use of color); ux.md 8.2 item 10 ("4 out of 5" shown).
- Fix: `web/src/components.js` `stars()`: draw unfilled stars as outlines (☆ or an SVG stroke), and add visible text "2 of 5" next to the stars.

#### M13. Keys has no "Get verified" section

- Screen: Keys, after a key exists. Viewports: all.
- Screenshot: `flow-keys-created-1280.png`.
- Evidence: the screen shows public key, storage, backup, and switch key. There is no way to see the identity attestors, nor the `perch.json` and DNS TXT values for the user's key. Persona task 3.3 "copy the exact `perch.json` content" fails.
- Rule broken: ux.md 8.6.6; identity.md 3.4.
- Fix: `web/src/screens/keys.js`: add "Get verified" as in ux.md 8.6.6. It lists the attestors from the active policy, and a domain field that shows both proofs with the key filled in and copy buttons.

### Minor

#### m1. Two search fields on Search; the header keeps a stale query

- Screen: Search, all screens. Viewports: all; worst at 360 px (2 inputs in the first 340 px).
- Screenshots: `search-360-light.png`, `explore-search-1280.png`, `crop-kettle-1280-a.png` (header shows "cobalt" on the kettle page).
- Rule: ux.md 5.2 ("On Search it is the only search box").
- Fix: `web/src/screens/search.js`: remove the page's text field; keep the Sort and Currency controls, bound to the header query. `web/src/app.js`: set `#topbar-q` from the route's `q` on every navigation, and clear it outside Search.

#### m2. Withdrawn listings are shown in full under "Where to buy"

- Screen: Product (kettle, Hearth & Trail €51.99). Viewports: all.
- Screenshot: `crop-kettle-1280-b.png`.
- Evidence: the withdrawn listing shows price, "In stock, New", and an active "Seller page" link. The heading "Where to buy (6)" counts it.
- Rule: T3; ux.md 8.2 item 7 ("Withdrawn by the seller on `<date>`" as one line).
- Fix: `web/src/screens/product.js`: render withdrawn listings as one line with a record link. Header: "6 listings: 5 current, 1 withdrawn".

#### m3. Clicking a histogram bar shows a different count than the bar

- Screen: Product (kettle). Viewport: 1280.
- Evidence: the bar "2 stars: 2" counts identity-verified reviewers. Clicking it sets "Stars: 2" with "Show reviews from: Everyone", which lists 3 reviews.
- Fix: `web/src/screens/product.js`: a click on the identity-verified histogram also sets the source filter to "Identity-verified people".

#### m4. Missing values read "None"

- Screen: Listing ("Ships to None", "Ships from None"), Author ("Website None", "About None", "Shop domains None"), Product facts ("MPN None"), record inspector ("Replaces None"). Viewports: all.
- Screenshots: `crop-author-mallory-1280.png`, `crop-record-1280.png`.
- Rule: T5. "None" reads as "ships nowhere".
- Fix: `web/src/components.js` `definitionList()`: render absent values as "Not stated" (listing and profile fields) or "—" with "No earlier version" (Replaces).

#### m5. Copy uses protocol words or is inaccurate

| Screen | Current | Proposed | Rule |
|---|---|---|---|
| Product rating, reviews | "author's log is forked" | "author's key has a conflicting history" | 12.1 rule 1; 12.3 |
| Product rating | "1 an older review by the same author" (Alice's 2nd key) | "1 replaced by a newer review from the same person" | 12.3; accuracy |
| Product rating | "1 text removed" | "1 removed by a node" | 12.3 |
| Reviews | "Receipt does not verify under the listing's seller" | "Receipt not accepted: not signed by the listing's seller" | 6.3 |
| Price history | "Show all 1 prices as a table" | "Show the price as a table" / "Show all N prices" | plural rules |
| Policy | "Clock tolerance (milliseconds)" | "Allowed clock difference (minutes)" | 8.8 |
| Policy | checkboxes `attest.identity`, `attest.domain`, `attest.revoke`; "Trust records up to number" | "Identity", "Shop domains", "Revocations"; "Ignore records after #" | 8.8; 12.1 |
| Policy | "Flags to hide a record: … a listing, product, or profile" | "… a listing or profile" (products have their own threshold) | accuracy |
| Reviews | (no label on reviews without a receipt) | "No receipt" | 6.2, 6.3 |
| Keys | "Stored in this browser's IndexedDB, unencrypted" | "Stored unencrypted in this browser for `<origin>`" | 12.1 rule 1 |
| Data | "Rejected 7" | "Invalid 7 (stored, ignored by views)". Invalid records are kept and relayed (4.4). | T1; accuracy |

Files: `web/src/components.js` (`EXCLUDED_TEXT`, `PURCHASE_TEXT`), `screens/product.js`, `chart.js`, `screens/policy.js`, `screens/keys.js`, `screens/data.js`.

#### m6. Small targets in review footers, fact sources, and Network

- Screens: Product (360 px: 20 nodes), Network ("Status" link).
- Evidence: axe `target-size`: "Record" 42.9 × 21 px, "Flag" 26.8 × 21 px, fact "record" 39.2 × 18 px, "Status" 38.5 × 18 px, without 24 px spacing.
- Rule: WCAG 2.5.8; ux.md 10.4.
- Fix: `web/styles.css` `.record-link`, `.quiet-link`: `display: inline-block; min-height: 24px; padding: 3px 4px;` and 8 px gap between siblings.

#### m7. At 360 px the search field is focused before the Menu button above it

- Screen: all. Viewport: 360.
- Screenshot: `focus-360-tab4.png`.
- Evidence: Tab order is logo (y = 15) → search input (y = 62) → Menu (y = 10).
- Rule: WCAG 2.4.3.
- Fix: `web/index.html`: put `<nav>` before the search form in the DOM. Use CSS grid areas to keep the wide layout.

#### m8. A GTIN with a wrong check digit gives no hint

- Screen: Search, query `2000090000017`.
- Evidence: "No products match "2000090000017"." The correct GTIN `2000090000018` finds the kettle.
- Rule: ux.md 8.1 (GTIN input).
- Fix: `web/src/screens/search.js`: for 8, 12, 13, or 14 digits with a failing check digit, add "This is not a valid GTIN: the last digit should be 8."

#### m9. Policy JSON block cannot be scrolled by keyboard

- Screen: Policy. Viewports: all.
- Evidence: axe `scrollable-region-focusable` on `#policy-json`.
- Fix: `web/src/screens/policy.js`: `tabindex="0"`, `role="region"`, `aria-label="Policy as JSON"`.

#### m10. Product facts show the 14-digit key form of the GTIN

- Screen: Product, "Product details". Viewports: all.
- Evidence: the header shows "GTIN 2000090000018" (C10). The facts list shows "GTIN 00200090000021" on Cobalt.
- Fix: `web/src/screens/product.js`: use the printed-GTIN helper from `format.js` in the facts list too.

#### m11. `maxlength` truncates pasted text silently and counts UTF-16 units

- Screen: Publish (title 200, review 10,000, note 2,000, and others).
- Evidence: `maxlength` cuts pasted text without a message. It counts UTF-16 code units, so a review with emoji is cut before 10,000 Unicode scalar values (contract 4.5 counts scalar values).
- Fix: `web/src/screens/publish.js`: remove `maxlength`. Show a counter using `[...s].length` and validate on submit (M9).

## 3. Persona tasks

| Persona | Task (ux.md section 3) | Result | Finding |
|---|---|---|---|
| Maya | Find a product by GTIN | Pass (`2000090000018` → kettle, "Searching by GTIN") | m8 for a wrong check digit |
| Maya | Verified-purchase mean and count | Pass ("4.5 from 2 reviews") | — |
| Maya | Show 1-star and 2-star reviews in ≤ 2 actions | Pass (star filter radios; histogram buttons; state kept in URL) | m3 |
| Maya | Lowest price from a domain-verified seller | Partial: the price card is right (€47.99), but the listing list starts with €9.99 and a lookalike shop | B1 |
| Maya | Rating on a shop page (extension) | Pass (29/29 extension checks; collapsed tab; "This site" line; prefix shown) | — |
| Tomás | Name excluded reviews and why | Pass for reasons; fail for policy values | M2, m5 |
| Tomás | Sources of conflicting facts | Pass ("2 sources, 1 other value", with author chips) | — |
| Tomás | Seller restriction and its numbers | Pass for Shady Imports (3 listings, 9 people); wrong for Mallory | M7 |
| Tomás | Open a hidden review, read flags and appeal | Reviews: pass. Appeal: fail (not shown anywhere) | M5 |
| Tomás | Price on a date and who stated it | Pass (table under each chart) | — |
| Aiko | Create a key and back it up | Pass (download `perch-key-<8 hex>.json`; confirmation gate works) | — |
| Aiko | Copy `perch.json` content | Fail | M13 |
| Aiko | Publish and change a listing | Pass after 3 submits | M9, M10 |
| Aiko | Issue a receipt | Pass (uppercase key normalized; copy and download) | — |
| Aiko | Find hidden listings and appeal | Partial: Author lists statuses; Appeal tab exists; appeals never displayed | M5 |
| Priya | Review without receipt, labeled | Partial: no "No receipt" label is shown | m5 |
| Priya | Check her record reached nodes | Partial: publish result says "Sending it to 1 connected node"; no per-node confirmation | M3 |
| Sam | Verify a record without Perch code | Fail: commands missing; "Body: None" | M8 |
| Sam | Compare the wasm hash with the release | Fail: only the same-server checksum file | M4 |
| Sam | Find a node that lacks records | Pass with 3 nodes ("Nodes agree", per-node Behind/Ahead) | — |
| Sam | Change the rules and see the effect | Pass (review threshold 10 un-hides the review; hash changes) | M3 (no banner) |

## 4. Trust rules on real data

| Rule | Result | Findings |
|---|---|---|
| T1 Provenance | Pass for facts, reviews, flags, prices (record links everywhere) | M8, m5 (Data "Rejected") |
| T2 How computed | Partial: exclusions inline; no policy values, hash, or time | M2, M7 |
| T3 Moderation visible | Reviews pass; listings show hidden content; appeals missing | M1, M5, m2 |
| T4 Precise badges | Domain badges pass; identity and status sentences only in `title`; checksum overclaims | M4, M6, M7 |
| T5 Absence stated | Rating "No verified purchases" passes; "None" fields fail | m4 |
| T6 Counts with means | Pass everywhere tested, including search and extension | — |
| T7 No dark patterns, explained order | Order explained in search; listing order favors unverified shops; no review step | B1, M10 |
| T8 Facts visible | Footer shows version, wasm hash, policy hash; no banners | M3, M4 |

## 5. Status of changes C1–C16

| # | Change | Status | Note |
|---|---|---|---|
| C1 | `--line-strong`, `--axis`, `--star-off` values | Done | Star shape issue: M12 |
| C2 | 3 chart series, chooser | Done | Default selection: B1 |
| C3 | Badge sentences | Partial | M6 |
| C4 | Publish buttons never disabled | Done | Gate notice shown, button enabled |
| C5 | Moderated search hits grouped | Done in code | Demo data has no hidden product; not exercised |
| C6 | `why` as sentences, "Show raw" | Done | — |
| C7 | Receipt share on Listing and Author | Done | "0 of 3 counted reviews … cite a valid receipt" |
| C8 | Histogram rows filter; star radios | Done | m3 |
| C9 | Image hash check | Done in code | Demo data has no images; not exercised |
| C10 | Printed GTIN | Partial | Header yes; facts no (m10) |
| C11 | Menu below 600 px | Done | "1/1" badge: M3; tab order: m7 |
| C12 | Extension collapsed tab, Alt+Shift+P, Escape | Done | Extension test |
| C13 | Panel only with exactly 1 GTIN | Done | Extension test |
| C14 | "This site" and privacy line | Done | Extension test |
| C15 | Prefix numbers and "Toolbar button only" | Done | Default is "Toolbar button only" (answers ux.md Q4) |
| C16 | Release checksum link | Not done | `release_page_url` is empty; M4 |

## 6. Summary

The app has the screens, data, and keyboard behavior the spec asks for. It renders 18 routes at 360 px without horizontal scroll, keeps focus visible and ordered, and passes axe except for 3 rules (color contrast, target size, keyboard-scrollable region). Most trust facts are present and correct on real records: provenance, exclusion reasons, domain badges, redaction notices, fork evidence, receipt checks, and policy effects.

The review found 1 blocker, 13 major, and 11 minor issues. The blocker is listing order: unverified and lookalike shops lead the product's listing list and its default price chart, which defeats domain verification (D14). Fixing it needs a small view change (`feeds_prices` and ordering in views §8.4) plus app grouping.

The majors cluster in 3 places:

- **Moderation display:** hidden listings are not collapsed, and appeals are never shown.
- **Explanations:** no policy values or hash for aggregates; badge sentences only in tooltips; a wrong restriction reason; a misleading checksum "matches"; missing banners for single-node and custom-policy states; record inspector errors.
- **Publishing:** errors one at a time, and no review step.

Two accessibility failures remain: contrast of not-counted items lowered by opacity, and star ratings shown by hue only.

All fixes name their files above. Two need contract or view changes, owned by analyst: `feeds_prices` (B1) and `restriction.reason` (M7).
