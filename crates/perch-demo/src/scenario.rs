//! The fixed demo scenario. Every view rule has at least one case here.
//!
//! Times are days after [`crate::BASE_TS`]; `policy.now` is day 270.
//!
//! | Part | Names | What it shows |
//! |---|---|---|
//! | Attestors | `attestor_a` (identity, revoke), `attestor_b` (domains, some identities) | The two trusted attestors. B is compromised after `attestor_b_honest_through_seq` and attests `sybil_b_0`, `sybil_b_1`, who post 5-star reviews of `puzzle`. |
//! | Untrusted attestor | `rogue_attestor` | Attests every `brigade_*` key; trusted by no policy. |
//! | Node operator | `node_op` | Publishes `redact` notices. |
//! | Verified sellers | `seller_good`, `seller_second`, `seller_flagged`, `seller_appeal`, `seller_restricted`, `seller_maker` | Business identities from A, domains from B. |
//! | Unverified sellers | `seller_unverified`, `seller_phish`, `time_traveler` | No attestation. `seller_phish` lists the cheapest `kettle` offer on `goodshop-deals.example`; `time_traveler` publishes a listing dated 2 days after `now`. |
//! | One person, two keys | `alice` + `alice_phone`; `seller_restricted` + `seller_restricted_alt`; `bob` + `bob_new`; `carol` + `carol_new` | Same `person` from attestor A. |
//! | Two attestors, one person | `judy` | Identities from A and B. |
//! | Fact conflicts | product `kettle` | `seller_good` and `ivan` (verified) vs. `seller_unverified` and `henry` (unverified) disagree on name, category, and `power`. `seller_good` later replaces its product record. |
//! | Price history | listing `kettle_good` | Three versions (49.99, 44.99, 47.99 EUR); observations by `crawler_verified` (counted) and `crawler_anon` (a fake 99.99 "was" price, excluded). |
//! | Currencies | `kettle_gbp` | A GBP offer next to EUR offers. |
//! | Verified purchase | reviews `kettle_alice` (verified), `mug_alice` (verified), `kettle_receipt_survives` (verified; its listing `kettle_second` is retracted later) | All six conditions hold. |
//! | Purchase failures | `kettle_wrong_product`, `kettle_other_buyer`, `kettle_tampered` (receipt_invalid), `kettle_unverified_seller`, `kettle_unverified_buyer`, `kettle_self` | One failing condition each. |
//! | Counting | `kettle_alice_phone` (superseded by the same person's later review), `kettle_self` (seller), `kettle_mallory` (forked), `kettle_retracted` (retracted), `kettle_frank` (unavailable), `kettle_carol` (replaced: 3 → 2 stars), `kettle_dave` (author's attestation expired) | Exclusion reasons. |
//! | Brigade | product `tent` | Unverified `brigade_*` keys post 1-star reviews; six verified people rate it 4–5 (one, `erin`, loses verification). |
//! | Hidden listing | `headphones_hidden` | Flags by `flagger_9..11` (counted); `brigade_0`, `brigade_1`, `dave` (unverified), and `seller_flagged` (self) do not count. |
//! | Restored listing | `headphones_restored` | 3 verified flags, an appeal, then 2 flag retractions. |
//! | Hidden review | `headphones_hidden_review` | 5 verified flags; flags by `seller_good` and `seller_appeal` (sellers of the product) do not count. Still counts in ratings. |
//! | Restriction | `seller_restricted` | Listings `lamp_shady`, `charger_shady`, `cable_shady` hidden by 9 distinct verified flaggers; `lamp_alt` by the same person's other key is marked restricted too. |
//! | One flag per person | `lamp_alt` | Flags by `alice` and `alice_phone` count once. |
//! | Flag on a flag | record `flag_on_flag` | Recorded; hides nothing. |
//! | Revocation | `bob` | Key stolen after `headphones_bob`; the thief's review and listing are ignored after `attest_revoke_bob`. |
//! | Expiry, retraction of attestations | `dave` (expires day 100), `erin` (retracted day 200) | Unverified at `now`. |
//! | Retirement | `carol` | `retire` with successor `carol_new`; a later review is ignored. |
//! | Fork | `mallory` | Two records at one seq; `fork_evidence`. A third record follows on one branch. |
//! | Redaction | `kettle_frank` (body removed, `redact` notice), `headphones_gina` (body removed, no notice), `headphones_judy` (notice, body present: no effect) | Body removal. |
//! | Invalid records | `invalid_listing_http`, `invalid_review_rating`, `invalid_product_gtin`, `invalid_replaces_author`, `invalid_retract_author`, `invalid_appeal_author`, `invalid_fixed_product` | One semantic failure each. |
//! | Unknown kinds | `unknown_wishlist`, `unknown_poll` | Stored, ignored by views. |
//! | Future record | `kettle_future` | `ts` 2 days after `now`. |

use perch_core::serde_json::{Value, json};

use crate::{BASE_TS, DAY, DEMO_NOW, Gen, Size, gtin};

const EXP: u64 = BASE_TS + 730 * DAY;
const EXP_SHORT: u64 = BASE_TS + 100 * DAY;

/// Number of brigade keys per size.
pub fn brigade_size(size: Size) -> usize {
    match size {
        Size::Small => 16,
        Size::Medium => 40,
        Size::Large => 60,
    }
}

/// Number of verified community flaggers (`flagger_0` …).
pub const FLAGGERS: usize = 12;

const fn d(day: u64) -> u64 {
    BASE_TS + day * DAY
}

pub(crate) fn plan(g: &mut Gen, size: Size) {
    let brigade = brigade_size(size);
    g.at(0, 0, setup_attestors);
    g.at(1, 0, move |g| setup_actors(g, brigade));
    g.at(3, 0, products);
    g.at(10, 0, listings);
    g.at(20, 0, observations_and_prices);
    g.at(24, 0, kettle_reviews);
    g.at(40, 0, other_reviews);
    g.at(150, 0, move |g| brigade_reviews(g, brigade));
    g.at(160, 0, stolen_key);
    g.at(200, 0, moderation);
    g.at(205, 0, redaction_and_compromise);
    g.at(209, 0, invalid_and_unknown);
    g.at(215, 0, late_events);
    g.at(230, 0, fork);
    g.at(50, 30, kettle_product_update);
}

fn setup_attestors(g: &mut Gen) {
    let a = g.ax("attestor_a");
    let b = g.ax("attestor_b");
    let rogue = g.ax("rogue_attestor");
    let node = g.ax("node_op");
    g.profile(a, "Demo Identity Verifier A", "attestor", Some("https://verifier-a.example/procedure"));
    g.profile(b, "Demo Verifier B", "attestor", Some("https://verifier-b.example/"));
    g.profile(rogue, "Instant Verification Service", "attestor", None);
    g.profile(node, "Demo Node Operator", "node", Some("https://node.example/"));
}

/// `(label, display name, legal name, domain, country, human)`.
const SELLERS: &[(&str, &str, Option<&str>, Option<&str>, &str, &str)] = &[
    ("seller_good", "Goodshop", Some("Goodshop GmbH"), Some("goodshop.example"), "DE", "goodshop"),
    ("seller_second", "Hearth & Trail", Some("Hearth & Trail Ltd"), Some("hearthandtrail.example"), "GB", "hearth"),
    ("seller_unverified", "Bargain Barn", None, None, "US", "barn"),
    ("seller_phish", "G00dshop Deals", None, None, "US", "phish"),
    ("seller_flagged", "Quick Gadgets", Some("Quick Gadgets BV"), Some("quickgadgets.example"), "NL", "quick"),
    ("seller_appeal", "Sound Corner", Some("Sound Corner SARL"), Some("soundcorner.example"), "FR", "sound"),
    ("seller_restricted", "Shady Imports", Some("Shady Imports LLC"), Some("shadyimports.example"), "US", "shady"),
    ("seller_restricted_alt", "Shady Imports Outlet", Some("Shady Imports LLC"), None, "US", "shady"),
    ("seller_maker", "Clay & Kiln", Some("Clay & Kiln Studio"), Some("clayandkiln.example"), "IE", "clay"),
    ("time_traveler", "Tomorrow Deals", None, None, "US", "tomorrow"),
];

/// `(label, display name, role, attested by A, human)`.
const PEOPLE: &[(&str, &str, &str, bool, &str)] = &[
    ("alice", "Alice", "buyer", true, "alice"),
    ("alice_phone", "Alice (phone)", "buyer", true, "alice"),
    ("bob", "Bob", "buyer", true, "bob"),
    ("bob_new", "Bob", "buyer", true, "bob"),
    ("carol", "Carol", "buyer", true, "carol"),
    ("dave", "Dave", "buyer", true, "dave"),
    ("erin", "Erin", "buyer", true, "erin"),
    ("frank", "Frank", "buyer", true, "frank"),
    ("gina", "Gina", "buyer", true, "gina"),
    ("henry", "Henry", "buyer", false, "henry"),
    ("ivan", "Ivan", "tester", true, "ivan"),
    ("judy", "Judy", "buyer", true, "judy"),
    ("mallory", "Mallory", "buyer", true, "mallory"),
    ("vandal", "xX_vandal_Xx", "other", false, "vandal"),
    ("crawler_verified", "Price Watch Crawler", "crawler", true, "pricewatch"),
    ("crawler_anon", "Anonymous Crawler", "crawler", false, "anon"),
];

fn setup_actors(g: &mut Gen, brigade: usize) {
    let a = g.ax("attestor_a");
    let b = g.ax("attestor_b");
    let rogue = g.ax("rogue_attestor");
    for &(label, name, legal, domain, country, human) in SELLERS {
        let s = g.ax(label);
        let url = domain.map(|dm| format!("https://{dm}/"));
        g.profile(s, name, "seller", url.as_deref());
        if let Some(legal) = legal {
            let r = g.attest_identity(a, "attestor_a", s, human, Some(legal), country, EXP);
            g.name(&format!("attest_identity_{label}"), &r);
        }
        if let Some(dm) = domain {
            let r = g.attest_domain(b, s, dm, EXP);
            g.name(&format!("attest_domain_{label}"), &r);
        }
    }
    for &(label, name, role, attested, human) in PEOPLE {
        let p = g.ax(label);
        g.profile(p, name, role, None);
        if attested {
            let exp = if label == "dave" { EXP_SHORT } else { EXP };
            let r = g.attest_identity(a, "attestor_a", p, human, None, "DE", exp);
            g.name(&format!("attest_identity_{label}"), &r);
        }
    }
    let judy = g.ax("judy");
    let r = g.attest_identity(b, "attestor_b", judy, "judy", None, "DE", EXP);
    g.name("attest_identity_judy_b", &r);
    for i in 0..crate::scenario::FLAGGERS {
        let f = g.ax(&format!("flagger_{i}"));
        g.profile(f, &format!("Community Member {i}"), "buyer", None);
        g.attest_identity(a, "attestor_a", f, &format!("flagger{i}"), None, "NL", EXP);
    }
    for i in 0..brigade {
        let k = g.ax(&format!("brigade_{i}"));
        if i % 2 == 0 {
            g.profile(k, &format!("Camper{}", 100 + i), "buyer", None);
        }
        g.attest_identity(rogue, "rogue_attestor", k, &format!("brigade{i}"), None, "US", EXP);
    }
}

fn products(g: &mut Gen) {
    let good = g.ax("seller_good");
    let unver = g.ax("seller_unverified");
    let henry = g.ax("henry");
    let ivan = g.ax("ivan");
    let kettle_gtin = gtin::make(13, "20", 9_000_001);
    let kettle = json!({
        "name": "Alder Kettle 1.7 L",
        "brand": "Alder",
        "gtin": kettle_gtin,
        "category": "kitchen/kettles",
        "attributes": { "capacity": "1.7 L", "power": "2200 W", "material": "stainless steel" },
        "images": [{ "url": "https://www.goodshop.example/img/alder-kettle.jpg",
                     "sha256": perch_core::ids::encode_hex(&perch_core::sha256(b"alder-kettle.jpg")) }],
        "description": "Electric kettle, 1.7 litres, automatic shut-off.",
    });
    let r = g.product("kettle", good, kettle, None);
    g.name("product_kettle_good", &r);
    let variant = json!({
        "name": "Alder Kettle 1.7L Stainless Steel 3000W",
        "brand": "Alder",
        "gtin": kettle_gtin,
        "category": "kitchen/appliances",
        "attributes": { "capacity": "1.7 L", "power": "3000 W" },
        "images": [{ "url": "https://bargainbarn.example/img/kettle.jpg" }],
    });
    g.product("kettle", unver, variant.clone(), None);
    g.product("kettle", henry, variant, None);
    g.product(
        "kettle",
        ivan,
        json!({ "name": "Alder Kettle 1.7 L", "brand": "Alder", "gtin": kettle_gtin, "category": "kitchen/kettles",
                "attributes": { "power": "2200 W" } }),
        None,
    );

    let appeal = g.ax("seller_appeal");
    g.product(
        "headphones",
        appeal,
        json!({ "name": "Cobalt H-300 Wireless Headphones", "brand": "Cobalt", "gtin": gtin::make(12, "2", 9_000_002),
                "category": "electronics/headphones", "attributes": { "battery": "30 h", "bluetooth": "5.3" } }),
        None,
    );
    let second = g.ax("seller_second");
    g.product(
        "tent",
        second,
        json!({ "name": "Fjord Trail 2-Person Tent", "brand": "Fjord", "gtin": gtin::make(13, "20", 9_000_003),
                "category": "outdoor/tents", "attributes": { "weight": "2.1 kg", "season": "3" } }),
        None,
    );
    let shady = g.ax("seller_restricted");
    g.product(
        "lamp",
        shady,
        json!({ "name": "Lumen LX-200 Desk Lamp", "brand": "Lumen", "mpn": "LX-200", "category": "home/lighting" }),
        None,
    );
    g.product(
        "charger",
        shady,
        json!({ "name": "Voltix 65 W USB-C Charger", "brand": "Voltix", "gtin": gtin::make(13, "20", 9_000_004),
                "category": "electronics/chargers" }),
        None,
    );
    g.product(
        "cable",
        shady,
        json!({ "name": "Voltix USB-C Cable 2 m", "brand": "Voltix", "mpn": "VX-C2M", "category": "electronics/cables" }),
        None,
    );
    let maker = g.ax("seller_maker");
    g.product(
        "mug",
        maker,
        json!({ "name": "Hand-thrown Mug, Blue Glaze", "brand": "Clay & Kiln", "category": "home/kitchen",
                "description": "Stoneware mug, about 350 ml. Each piece differs slightly." }),
        None,
    );
    g.product(
        "puzzle",
        good,
        json!({ "name": "Kestrel Wooden Puzzle 500", "brand": "Kestrel", "gtin": gtin::make(8, "20", 91),
                "category": "toys/puzzles" }),
        None,
    );
}

fn listings(g: &mut Gen) {
    let good = g.ax("seller_good");
    let second = g.ax("seller_second");
    let unver = g.ax("seller_unverified");
    let phish = g.ax("seller_phish");
    let flagged = g.ax("seller_flagged");
    let appeal = g.ax("seller_appeal");
    let shady = g.ax("seller_restricted");
    let shady_alt = g.ax("seller_restricted_alt");
    let maker = g.ax("seller_maker");
    let tt = g.ax("time_traveler");

    g.listing("kettle_good", good, "kettle", "https://www.goodshop.example/p/alder-kettle-1-7", 4999, "EUR", Some("Alder Kettle 1.7 L, stainless"));
    g.listing("kettle_second", second, "kettle", "https://hearthandtrail.example/alder-kettle", 5299, "EUR", None);
    g.listing("kettle_gbp", second, "kettle", "https://hearthandtrail.example/uk/alder-kettle", 4299, "GBP", None);
    g.listing("kettle_unverified", unver, "kettle", "https://bargainbarn.example/kettle", 3999, "EUR", None);
    g.listing("kettle_phish", phish, "kettle", "https://goodshop-deals.example/alder-kettle-sale", 1999, "EUR", Some("Alder Kettle 1.7 L - 60% OFF TODAY"));
    g.listing("headphones_good", good, "headphones", "https://www.goodshop.example/p/cobalt-h300", 12999, "EUR", None);
    g.listing("headphones_hidden", flagged, "headphones", "https://www.quickgadgets.example/h300", 8999, "EUR", Some("Cobalt H300 headphones ORIGINAL"));
    g.listing("headphones_restored", appeal, "headphones", "https://soundcorner.example/cobalt-h300", 11999, "EUR", None);
    g.listing("tent_second", second, "tent", "https://hearthandtrail.example/fjord-trail-2", 19999, "EUR", None);
    g.listing("lamp_shady", shady, "lamp", "https://shadyimports.example/lx200", 2999, "EUR", None);
    g.listing("charger_shady", shady, "charger", "https://shadyimports.example/voltix-65w", 1999, "EUR", None);
    g.listing("cable_shady", shady, "cable", "https://shadyimports.example/vx-c2m", 499, "EUR", None);
    g.listing("lamp_alt", shady_alt, "lamp", "https://outlet.shadyimports.example/lx200", 2799, "EUR", None);
    g.listing("mug_maker", maker, "mug", "https://clayandkiln.example/mugs/blue", 3400, "EUR", None);
    g.listing("puzzle_good", good, "puzzle", "https://www.goodshop.example/p/kestrel-puzzle-500", 1999, "EUR", None);

    let kettle = g.key("kettle");
    let r = g.emit_at(
        tt,
        DEMO_NOW + 2 * DAY,
        "listing",
        json!({ "product": kettle.as_str(), "url": "https://tomorrow-deals.example/kettle",
                "price": { "amount": 999, "currency": "EUR" }, "availability": "preorder", "condition": "new" }),
    );
    g.name("kettle_future", &r);
}

fn observations_and_prices(g: &mut Gen) {
    let cv = g.ax("crawler_verified");
    let ca = g.ax("crawler_anon");
    g.at(30, 0, move |g| {
        let t = g.now();
        g.observe(cv, "kettle_good", 4999, t - 3_600_000);
    });
    g.at(40, 0, move |g| {
        let t = g.now();
        g.observe(cv, "kettle_phish", 1999, t - 600_000);
    });
    g.at(45, 0, move |g| {
        let t = g.now();
        g.observe(cv, "headphones_good", 12999, t - 600_000);
    });
    g.at(60, 0, |g| {
        g.reprice("kettle_good", 4499, "in_stock");
    });
    g.at(65, 0, |g| {
        g.reprice("kettle_second", 5199, "in_stock");
    });
    g.at(80, 0, |g| {
        g.reprice("headphones_good", 11999, "in_stock");
    });
    g.at(90, 0, move |g| {
        let t = g.now();
        g.observe(cv, "kettle_good", 4499, t - 3_600_000);
    });
    g.at(100, 0, move |g| {
        let t = g.now();
        let r = g.observe(ca, "kettle_good", 9999, t - 60_000);
        g.name("observation_fake_was_price", &r);
    });
    g.at(120, 0, |g| {
        g.reprice("kettle_good", 4799, "in_stock");
    });
    g.at(150, 0, move |g| {
        let t = g.now();
        g.observe(cv, "kettle_good", 4799, t - 3_600_000);
    });
}

fn kettle_reviews(g: &mut Gen) {
    let alice = g.ax("alice");
    let alice_phone = g.ax("alice_phone");
    g.review("kettle_alice_phone", alice_phone, "kettle", 3, None, "First impressions: boils fast, lid feels loose.", None, None);
    g.at(25, 0, move |g| {
        let rc = g.receipt("kettle_good", alice, d(22));
        g.receipts.insert("alice_kettle".into(), rc.clone());
        g.review("kettle_alice", alice, "kettle", 5, Some("Quiet and fast"), "Boils a full kettle in under four minutes. Lid is fine after a week.", Some("kettle_good"), Some(&rc));
    });
    g.at(27, 0, |g| {
        let judy = g.ax("judy");
        let rc = g.receipts["alice_kettle"].clone();
        g.review("kettle_other_buyer", judy, "kettle", 5, None, "Great kettle.", Some("kettle_good"), Some(&rc));
    });
    g.at(28, 0, |g| {
        let ivan = g.ax("ivan");
        let rc = g.receipt("headphones_good", ivan, d(26));
        g.review("kettle_wrong_product", ivan, "kettle", 2, None, "Limescale after a month.", Some("headphones_good"), Some(&rc));
    });
    g.at(30, 0, |g| {
        let carol = g.ax("carol");
        let rc = g.receipt("kettle_unverified", carol, d(27));
        let r = g.review("kettle_carol", carol, "kettle", 3, None, "Works. Switch is stiff.", Some("kettle_unverified"), Some(&rc));
        g.at(95, 0, move |g| {
            let key = g.key("kettle");
            let carol = g.ax("carol");
            g.review_key("", carol, &key, 2, Some("Switch broke"), "Update: the switch broke after three months.", Some("kettle_unverified"), Some(&rc), Some(r.id()));
        });
    });
    g.at(31, 0, |g| {
        let henry = g.ax("henry");
        let rc = g.receipt("kettle_good", henry, d(29));
        g.review("kettle_unverified_buyer", henry, "kettle", 4, None, "Good kettle, fast delivery.", Some("kettle_good"), Some(&rc));
    });
    g.at(32, 0, |g| {
        let good = g.ax("seller_good");
        let rc = g.receipt("kettle_good", good, d(31));
        g.review("kettle_self", good, "kettle", 5, Some("Our best seller"), "Customers love it.", Some("kettle_good"), Some(&rc));
    });
    g.at(33, 0, |g| {
        let f9 = g.ax("flagger_9");
        let mut rc = g.receipt("kettle_good", f9, d(30));
        rc.purchased_at += 1;
        g.review("kettle_tampered", f9, "kettle", 5, None, "Perfect.", Some("kettle_good"), Some(&rc));
    });
    g.at(34, 0, |g| {
        let f10 = g.ax("flagger_10");
        let rc = g.receipt("kettle_second", f10, d(32));
        g.review("kettle_receipt_survives", f10, "kettle", 4, None, "Solid. The seller later removed the listing.", Some("kettle_second"), Some(&rc));
    });
    g.at(35, 0, |g| {
        let frank = g.ax("frank");
        g.review("kettle_frank", frank, "kettle", 1, Some("Terrible"), "Broke on day one. Call me on +1 555 0100 for photos, I live at 12 Example Street.", None, None);
    });
    g.at(36, 0, |g| {
        let m = g.ax("mallory");
        g.review("kettle_mallory", m, "kettle", 2, None, "Meh.", None, None);
    });
    g.at(37, 0, |g| {
        let dave = g.ax("dave");
        g.review("kettle_dave", dave, "kettle", 3, None, "Average.", None, None);
    });
    g.at(38, 0, |g| {
        let f11 = g.ax("flagger_11");
        let r = g.review("kettle_retracted", f11, "kettle", 1, None, "Wrong product review, sorry.", None, None);
        g.at(150, 0, move |g| {
            let f11 = g.ax("flagger_11");
            let rt = g.retract(f11, r.id());
            g.name("retract_kettle_review", &rt);
        });
    });
}

fn other_reviews(g: &mut Gen) {
    let ivan = g.ax("ivan");
    g.review("headphones_hidden_review", ivan, "headphones", 1, Some("Broke in two days"), "Left ear cup cracked after two days of normal use.", None, None);
    g.at(42, 0, |g| {
        let gina = g.ax("gina");
        let r = g.review("headphones_gina", gina, "headphones", 4, None, "Comfortable for long flights.", None, None);
        g.strip_body(r.id());
    });
    g.at(43, 0, |g| {
        let bob = g.ax("bob");
        let r = g.review("headphones_bob", bob, "headphones", 5, None, "Great sound for the price.", None, None);
        g.name("bob_last_honest", &r);
    });
    g.at(44, 0, |g| {
        let judy = g.ax("judy");
        g.review("headphones_judy", judy, "headphones", 4, None, "Good battery life.", None, None);
    });
    g.at(50, 0, |g| {
        for (label, rating, text) in [
            ("alice", 5, "Dry after a night of heavy rain."),
            ("ivan", 4, "Easy to pitch; a bit heavy."),
            ("judy", 5, "Roomy for two with packs."),
            ("flagger_0", 4, "Good tent, poles feel solid."),
            ("flagger_1", 5, "Third season with it, no problems."),
            ("erin", 2, "Zipper snagged often."),
        ] {
            let a = g.ax(label);
            g.review(&format!("tent_{label}"), a, "tent", rating, None, text, None, None);
        }
    });
    g.at(60, 0, |g| {
        let alice = g.ax("alice");
        let rc = g.receipt("mug_maker", alice, d(55));
        g.review("mug_alice", alice, "mug", 5, None, "Beautiful glaze, holds heat well.", Some("mug_maker"), Some(&rc));
    });
    g.at(70, 0, |g| {
        let f2 = g.ax("flagger_2");
        g.review("puzzle_flagger_2", f2, "puzzle", 3, None, "Pieces fit loosely.", None, None);
        let f3 = g.ax("flagger_3");
        g.review("charger_flagger_3", f3, "charger", 1, Some("Overheats"), "Gets too hot to touch at 65 W.", Some("charger_shady"), None);
    });
}

fn brigade_reviews(g: &mut Gen, n: usize) {
    const TEXTS: &[&str] = &["Terrible tent.", "Leaks. Avoid.", "Worst purchase ever.", "Do not buy.", "Fell apart."];
    for i in 0..n {
        let k = g.ax(&format!("brigade_{i}"));
        let title = if i % 3 == 0 { Some("AVOID") } else { None };
        let text = TEXTS[i % TEXTS.len()];
        g.review(&format!("tent_brigade_{i}"), k, "tent", 1, title, text, None, None);
    }
}

fn stolen_key(g: &mut Gen) {
    // A thief uses bob's key after `bob_last_honest`.
    let bob = g.ax("bob");
    let r = g.review("headphones_bob_thief", bob, "headphones", 1, None, "Better ones at cheap-audio.example!!!", None, None);
    g.name("bob_thief_review", &r);
    let tent = g.key("tent");
    let l = g.emit(
        bob,
        "listing",
        json!({ "product": tent.as_str(), "url": "https://cheap-tents.example/fjord", "price": { "amount": 999, "currency": "EUR" },
                "availability": "in_stock", "condition": "new" }),
        None,
    );
    g.name("bob_thief_listing", &l);
    g.at(170, 0, |g| {
        let a = g.ax("attestor_a");
        let bob = g.ax("bob");
        let through = g.named("bob_last_honest").seq();
        let r = g.emit(a, "attest.revoke", json!({ "subject": g.id(bob).to_hex(), "valid_through_seq": through }), None);
        g.name("attest_revoke_bob", &r);
    });
    g.at(175, 0, |g| {
        let bob_new = g.ax("bob_new");
        g.review("lamp_bob_new", bob_new, "lamp", 2, None, "Flickers at low brightness.", None, None);
    });
}

fn moderation(g: &mut Gen) {
    let a = g.ax("attestor_a");
    let erin_att = g.named("attest_identity_erin").id();
    let r = g.retract(a, erin_att);
    g.name("retract_attest_erin", &r);

    // Hidden listing.
    g.at(201, 0, |g| {
        let target = g.listing_info("headphones_hidden").root;
        for (i, reason) in [(9, "counterfeit"), (10, "counterfeit"), (11, "fraud")] {
            let f = g.ax(&format!("flagger_{i}"));
            g.flag(f, target, reason, Some("Box and serial number do not match the brand's."));
        }
        for label in ["brigade_0", "brigade_1", "dave", "seller_flagged"] {
            let f = g.ax(label);
            let r = g.flag(f, target, "other", None);
            g.name(&format!("flag_headphones_hidden_{label}"), &r);
        }
    });
    // Restored listing: flags, appeal, retractions.
    g.at(202, 0, |g| {
        let target = g.listing_info("headphones_restored").root;
        for i in 0..3 {
            let f = g.ax(&format!("flagger_{i}"));
            let r = g.flag(f, target, "misleading", Some("Looks like a grey import."));
            g.name(&format!("flag_restored_{i}"), &r);
        }
    });
    g.at(203, 0, |g| {
        let target = g.listing_info("headphones_restored").root;
        let s = g.ax("seller_appeal");
        let r = g.emit(s, "appeal", json!({ "target": target.to_hex(),
            "note": "These are genuine EU units. Invoice from Cobalt available on request." }), None);
        g.name("appeal_restored", &r);
    });
    g.at(210, 0, |g| {
        for i in 0..2 {
            let f = g.ax(&format!("flagger_{i}"));
            let fl = g.named(&format!("flag_restored_{i}")).id();
            g.retract(f, fl);
        }
    });
    // Restriction: 3 listings × 3 distinct verified flaggers.
    g.at(204, 0, |g| {
        for (j, l) in ["lamp_shady", "charger_shady", "cable_shady"].iter().enumerate() {
            let target = g.listing_info(l).root;
            for i in 0..3 {
                let f = g.ax(&format!("flagger_{}", j * 3 + i));
                let r = g.flag(f, target, "counterfeit", None);
                if j == 0 && i == 0 {
                    g.name("flag_lamp_flagger_0", &r);
                }
            }
        }
    });
    // Hidden review, plus product-seller flags that never count.
    g.at(205, 0, |g| {
        let target = g.reviews["headphones_hidden_review"].id();
        for i in 3..8 {
            let f = g.ax(&format!("flagger_{i}"));
            g.flag(f, target, "spam", None);
        }
        for label in ["seller_good", "seller_appeal"] {
            let f = g.ax(label);
            let r = g.flag(f, target, "misleading", Some("Customer misused the product."));
            g.name(&format!("flag_review_{label}"), &r);
        }
    });
    // One person, two keys: counts once.
    g.at(207, 0, |g| {
        let target = g.listing_info("lamp_alt").root;
        for label in ["alice", "alice_phone"] {
            let f = g.ax(label);
            g.flag(f, target, "counterfeit", None);
        }
    });
    // A flag on a flag hides nothing.
    g.at(208, 0, |g| {
        let v = g.ax("vandal");
        let target = g.named("flag_lamp_flagger_0").id();
        let r = g.flag(v, target, "spam", None);
        g.name("flag_on_flag", &r);
    });
}

fn redaction_and_compromise(g: &mut Gen) {
    let node = g.ax("node_op");
    let frank = g.reviews["kettle_frank"].id();
    let r = g.emit(node, "redact", json!({ "target": frank.to_hex(), "reason": "personal_data",
        "note": "Removed a phone number and a home address." }), None);
    g.name("redact_frank", &r);
    g.strip_body(frank);
    let judy = g.reviews["headphones_judy"].id();
    let r = g.emit(node, "redact", json!({ "target": judy.to_hex(), "reason": "other" }), None);
    g.name("redact_judy_body_present", &r);

    // Attestor B is compromised from here on.
    let b = g.ax("attestor_b");
    g.attestor_b_honest_through_seq = g.author(b).next_seq() - 1;
    for i in 0..2 {
        let s = g.ax(&format!("sybil_b_{i}"));
        g.profile(s, &format!("Puzzle Fan {i}"), "buyer", None);
        let r = g.attest_identity(b, "attestor_b", s, &format!("sybil{i}"), None, "US", EXP);
        g.name(&format!("attest_identity_sybil_b_{i}"), &r);
    }
    g.at(206, 0, |g| {
        for i in 0..2 {
            let s = g.ax(&format!("sybil_b_{i}"));
            g.review(&format!("puzzle_sybil_b_{i}"), s, "puzzle", 5, None, "Amazing puzzle!!!", None, None);
        }
    });
}

fn invalid_and_unknown(g: &mut Gen) {
    let unver = g.ax("seller_unverified");
    let henry = g.ax("henry");
    let vandal = g.ax("vandal");
    let headphones = g.key("headphones");
    let tent = g.key("tent");

    let r = g.emit(unver, "listing", json!({ "product": headphones.as_str(), "url": "http://bargainbarn.example/h300",
        "price": { "amount": 5000, "currency": "EUR" }, "availability": "in_stock", "condition": "used" }), None);
    g.name("invalid_listing_http", &r);
    let r = g.emit(henry, "review", json!({ "product": tent.as_str(), "rating": 6, "text": "Six stars!" }), None);
    g.name("invalid_review_rating", &r);
    let bad = gtin::corrupt(&gtin::make(13, "20", 9_000_005));
    let r = g.emit(vandal, "product", json!({ "name": "Fake Product", "brand": "Nobrand", "gtin": bad }), None);
    g.name("invalid_product_gtin", &r);
    let alice_review = g.reviews["kettle_alice"].id();
    let kettle = g.key("kettle");
    let r = g.emit(vandal, "review", json!({ "product": kettle.as_str(), "rating": 1, "text": "Overwritten." }), Some(alice_review));
    g.name("invalid_replaces_author", &r);
    let kg = g.listing_info("kettle_good").root;
    let r = g.retract(vandal, kg);
    g.name("invalid_retract_author", &r);
    let hh = g.listing_info("headphones_hidden").root;
    let r = g.emit(vandal, "appeal", json!({ "target": hh.to_hex(), "note": "Unhide this." }), None);
    g.name("invalid_appeal_author", &r);

    g.at(210, 0, |g| {
        // A version of kettle_good that moves the listing to another product.
        let mut info = g.listing_info("kettle_good").clone();
        info.product = g.key("headphones");
        let r = g.emit(info.seller, "listing", info.body(), Some(info.latest));
        g.name("invalid_fixed_product", &r);
    });
    g.at(212, 0, |g| {
        let alice = g.ax("alice");
        let kettle = g.key("kettle");
        let r = g.emit(alice, "x.wishlist", json!({ "items": [kettle.as_str()], "public": true }), None);
        g.name("unknown_wishlist", &r);
        let vandal = g.ax("vandal");
        let r = g.emit(vandal, "poll", json!({ "question": "Best kettle?", "options": ["Alder", "Other"] }), None);
        g.name("unknown_poll", &r);
    });
}

fn late_events(g: &mut Gen) {
    let second = g.ax("seller_second");
    let root = g.listing_info("kettle_second").root;
    let r = g.retract(second, root);
    g.name("retract_kettle_second", &r);
    g.at(219, 0, |g| {
        let a = g.ax("attestor_a");
        let cn = g.ax("carol_new");
        g.profile(cn, "Carol", "buyer", None);
        g.attest_identity(a, "attestor_a", cn, "carol", None, "DE", EXP);
    });
    g.at(220, 0, |g| {
        let carol = g.ax("carol");
        let cn = g.ax("carol_new");
        let r = g.emit(carol, "retire", json!({ "successor": g.id(cn).to_hex() }), None);
        g.name("retire_carol", &r);
    });
    g.at(225, 0, |g| {
        let carol = g.ax("carol");
        g.review("tent_carol_after_retire", carol, "tent", 1, None, "Posted after retiring.", None, None);
    });
}

fn kettle_product_update(g: &mut Gen) {
    // Product record update by seller_good (chain with replaces).
    let good = g.ax("seller_good");
    let root = g.named("product_kettle_good").clone();
    let mut body: Value = Value::Object(root.body().expect("body").clone());
    body["description"] = Value::String("Electric kettle, 1.7 litres, automatic shut-off, limescale filter.".into());
    body["attributes"]["color"] = Value::String("steel".into());
    let r = g.product("kettle", good, body, Some(root.id()));
    g.name("product_kettle_good_v2", &r);
}

fn fork(g: &mut Gen) {
    let m = g.ax("mallory");
    let tent = g.key("tent");
    let a = g.review("tent_mallory", m, "tent", 1, None, "Tore on the first night.", None, None);
    let b = g.emit_fork(m, "review", json!({ "product": tent.as_str(), "rating": 5, "text": "Best tent ever." }));
    let pair = perch_core::EvidencePair::new(a, b).expect("fork evidence");
    let [x, y] = pair.records();
    g.fork_evidence = Some([x.clone(), y.clone()]);
    g.at(231, 0, |g| {
        let m = g.ax("mallory");
        g.profile(m, "Mallory", "buyer", None);
    });
}
