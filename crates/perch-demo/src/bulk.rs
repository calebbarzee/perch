//! Generated bulk data: many sellers, buyers, products, listings, price
//! changes, observations, and reviews, scaled by [`Size`].

use std::collections::BTreeSet;

use perch_core::serde_json::{Value, json};
use perch_core::{ProductKey, RecordId};

use crate::words::{self, BRANDS, CATEGORIES, COLORS, COUNTRIES, FIRST_NAMES, REVIEW_TEXT, REVIEW_TITLES, SHOP_ADJ, SHOP_NOUN};
use crate::{Ax, BASE_TS, DAY, Gen, Size, gtin};

const EXP: u64 = BASE_TS + 730 * DAY;

#[derive(Debug, Clone, Copy)]
struct Params {
    sellers: usize,
    buyers: usize,
    crawlers: usize,
    products: usize,
    /// Mean reviews per buyer.
    reviews_per_buyer: u64,
}

impl Params {
    fn for_size(size: Size) -> Params {
        match size {
            Size::Small => Params { sellers: 2, buyers: 8, crawlers: 1, products: 4, reviews_per_buyer: 3 },
            Size::Medium => Params { sellers: 12, buyers: 260, crawlers: 3, products: 220, reviews_per_buyer: 7 },
            Size::Large => Params { sellers: 200, buyers: 5_000, crawlers: 10, products: 4_400, reviews_per_buyer: 8 },
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Seller {
    ax: Ax,
    domain: String,
    currency: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct Product {
    key: ProductKey,
    name: String,
    quality: u8,
    base_eur: u64,
    listings: Vec<String>,
}

/// Bulk state kept in [`Gen`] between events.
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    sellers: Vec<Seller>,
    buyers: Vec<(Ax, bool)>,
    crawlers: Vec<Ax>,
    products: Vec<Product>,
}

pub(crate) fn plan(g: &mut Gen, size: Size) {
    let p = Params::for_size(size);
    g.at(2, 0, move |g| actors(g, p));
    g.at(4, 0, move |g| products(g, p));
    g.at(12, 0, move |g| listings(g, p));
}

fn actors(g: &mut Gen, p: Params) {
    let a = g.ax("attestor_a");
    let b = g.ax("attestor_b");
    for i in 0..p.sellers {
        let s = g.ax(&format!("~s{i}"));
        let name = format!("{} {} {}", SHOP_ADJ[i % SHOP_ADJ.len()], SHOP_NOUN[(i / SHOP_ADJ.len()) % SHOP_NOUN.len()], i + 1);
        let domain = format!("{}.example", words::slug(&name));
        let currency = match g.rng.below(20) {
            0..=11 => "EUR",
            12..=16 => "USD",
            _ => "GBP",
        };
        let country = match currency {
            "USD" => "US",
            "GBP" => "GB",
            _ => *g.rng.pick(&COUNTRIES[..6]),
        };
        g.profile(s, &name, "seller", Some(&format!("https://{domain}/")));
        if g.rng.chance(9, 10) {
            g.attest_identity(a, "attestor_a", s, &format!("~seller{i}"), Some(&format!("{name} Ltd")), country, EXP);
            g.attest_domain(b, s, &domain, EXP);
        }
        g.bulk.sellers.push(Seller { ax: s, domain, currency });
    }
    for i in 0..p.buyers {
        let u = g.ax(&format!("~b{i}"));
        if g.rng.chance(7, 10) {
            let name = format!("{} {}.", FIRST_NAMES[i % FIRST_NAMES.len()], (b'A' + (i / FIRST_NAMES.len() % 26) as u8) as char);
            g.profile(u, &name, "buyer", None);
        }
        let verified = g.rng.chance(7, 10);
        if verified {
            let country = *g.rng.pick(COUNTRIES);
            g.attest_identity(a, "attestor_a", u, &format!("~buyer{i}"), None, country, EXP);
            if g.rng.chance(1, 20) {
                g.attest_identity(b, "attestor_b", u, &format!("~buyer{i}"), None, country, EXP);
            }
        }
        g.bulk.buyers.push((u, verified));
    }
    for i in 0..p.crawlers {
        let c = g.ax(&format!("~c{i}"));
        g.profile(c, &format!("Price Crawler {}", i + 1), "crawler", None);
        if i % 2 == 0 {
            g.attest_identity(a, "attestor_a", c, &format!("~crawler{i}"), None, "DE", EXP);
        }
        g.bulk.crawlers.push(c);
    }
}

fn products(g: &mut Gen, p: Params) {
    for i in 0..p.products {
        let (category, types, lo, hi) = CATEGORIES[g.rng.below(CATEGORIES.len() as u64) as usize];
        let brand = *g.rng.pick(BRANDS);
        let item = *g.rng.pick(types);
        let model = format!("{}-{}", (b'A' + g.rng.below(26) as u8) as char, 100 + i);
        let color = *g.rng.pick(COLORS);
        let name = format!("{brand} {item} {model}");
        let mut body = json!({
            "name": name,
            "brand": brand,
            "category": category,
            "attributes": { "color": color },
        });
        let ident = g.rng.below(100);
        let has_gtin = ident < 80;
        if has_gtin {
            let n = i as u64 + 1;
            let g_ = match g.rng.below(20) {
                0..=13 => gtin::make(13, "20", n),
                14..=17 => gtin::make(12, "2", n),
                18 => gtin::make(8, "2", n % 100_000),
                _ => gtin::make(14, "120", n),
            };
            body["gtin"] = Value::String(g_);
            if g.rng.chance(1, 2) {
                body["mpn"] = Value::String(model.clone());
            }
        } else if ident < 92 {
            body["mpn"] = Value::String(model.clone());
        }
        if g.rng.chance(1, 3) {
            body["description"] = Value::String(format!("{item} by {brand}, model {model}, colour {}.", color.to_lowercase()));
        }
        let author = author_for_product(g);
        let pname = format!("~p{i}");
        let r = g.product(&pname, author, body.clone(), None);
        let key = g.key(&pname);
        // A second record with a variant name from another author.
        if (has_gtin || ident < 92) && g.rng.chance(1, 10) {
            let other = author_for_product(g);
            if other != author {
                let mut v = body.clone();
                v["name"] = Value::String(format!("{brand} {} {model} ({color})", item.to_lowercase()));
                if g.rng.chance(1, 2) {
                    v["category"] = Value::String(category.split('/').next().unwrap_or(category).to_owned());
                }
                g.product(&pname, other, v, None);
            }
        }
        let _ = r;
        let base = g.rng.range(lo, hi) / 100 * 100 + 99;
        let quality = [2u8, 3, 4, 4, 4, 5, 5][g.rng.below(7) as usize];
        g.bulk.products.push(Product { key, name, quality, base_eur: base, listings: Vec::new() });
    }
}

fn author_for_product(g: &mut Gen) -> Ax {
    if g.rng.chance(7, 10) || g.bulk.buyers.is_empty() {
        let i = g.rng.below(g.bulk.sellers.len() as u64) as usize;
        g.bulk.sellers[i].ax
    } else {
        let i = g.rng.below(g.bulk.buyers.len() as u64) as usize;
        g.bulk.buyers[i].0
    }
}

fn convert(eur: u64, currency: &str) -> u64 {
    let v = match currency {
        "USD" => eur * 110 / 100,
        "GBP" => eur * 86 / 100,
        _ => eur,
    };
    (v / 100).max(1) * 100 - 1
}

fn availability(g: &mut Gen) -> &'static str {
    match g.rng.below(100) {
        0..=84 => "in_stock",
        85..=92 => "out_of_stock",
        93..=96 => "preorder",
        _ => "discontinued",
    }
}

/// A random time in days 20–190.
fn activity_ts(g: &mut Gen) -> u64 {
    BASE_TS + 20 * DAY + g.rng.below(170 * DAY)
}

fn listings(g: &mut Gen, p: Params) {
    let nsellers = g.bulk.sellers.len();
    for i in 0..g.bulk.products.len() {
        let k = (g.rng.range(1, 4) as usize).min(nsellers);
        let mut chosen = BTreeSet::new();
        while chosen.len() < k {
            chosen.insert(g.rng.below(nsellers as u64) as usize);
        }
        for (j, si) in chosen.into_iter().enumerate() {
            let s = g.bulk.sellers[si].clone();
            let prod = g.bulk.products[i].clone();
            let factor = g.rng.range(85, 115);
            let amount = convert(prod.base_eur * factor / 100, s.currency);
            let lname = format!("~l{i}_{j}");
            let url = format!("https://www.{}/p/{}", s.domain, words::slug(&prod.name));
            g.products.entry(format!("~p{i}")).or_insert(prod.key.clone());
            g.listing(&lname, s.ax, &format!("~p{i}"), &url, amount, s.currency, Some(&prod.name));
            if g.rng.chance(1, 10) {
                let mut info = g.listings[&lname].clone();
                info.condition = if g.rng.chance(1, 2) { "used" } else { "refurbished" };
                g.listings.insert(lname.clone(), info);
            }
            g.bulk.products[i].listings.push(lname.clone());
            // Price changes and observations.
            for _ in 0..g.rng.below(4) {
                let ts = activity_ts(g);
                let lname = lname.clone();
                g.at_ts(ts, move |g| {
                    let cur = g.listing_info(&lname).amount;
                    let f = g.rng.range(85, 110);
                    let av = availability(g);
                    g.reprice(&lname, (cur * f / 100).max(100) / 100 * 100 - 1, av);
                });
            }
            for _ in 0..g.rng.below(4) {
                let ts = activity_ts(g);
                let lname = lname.clone();
                g.at_ts(ts, move |g| {
                    let ci = g.rng.below(g.bulk.crawlers.len() as u64) as usize;
                    let c = g.bulk.crawlers[ci];
                    let cur = g.listing_info(&lname).amount;
                    let amount = if g.rng.chance(85, 100) { cur } else { cur * g.rng.range(95, 105) / 100 };
                    let t = g.now();
                    let at = t - g.rng.below(6 * 3_600_000).min(t - BASE_TS);
                    g.observe(c, &lname, amount, at);
                });
            }
            if g.rng.chance(1, 100) {
                let ts = BASE_TS + 100 * DAY + g.rng.below(90 * DAY);
                let lname = lname.clone();
                g.at_ts(ts, move |g| {
                    if let Some(f) = random_verified_buyer(g) {
                        let root = g.listing_info(&lname).root;
                        let reason = *g.rng.pick(words::FLAG_REASONS);
                        g.flag(f, root, reason, None);
                    }
                });
            }
        }
    }
    // Reviews.
    let n = g.bulk.products.len() as u64;
    for bi in 0..g.bulk.buyers.len() {
        let r = g.rng.range(1, 2 * p.reviews_per_buyer - 1).min(n);
        let mut chosen = BTreeSet::new();
        let mut tries = 0;
        while (chosen.len() as u64) < r && tries < 4 * r {
            // Skewed popularity: low indices are reviewed more often.
            let x = (u128::from(g.rng.below(n)) * u128::from(g.rng.below(n)) / u128::from(n)) as usize;
            chosen.insert(x);
            tries += 1;
        }
        for pi in chosen {
            let ts = activity_ts(g);
            g.at_ts(ts, move |g| review(g, bi, pi));
        }
    }
}

fn random_verified_buyer(g: &mut Gen) -> Option<Ax> {
    for _ in 0..8 {
        let i = g.rng.below(g.bulk.buyers.len() as u64) as usize;
        if g.bulk.buyers[i].1 {
            return Some(g.bulk.buyers[i].0);
        }
    }
    None
}

fn rating(g: &mut Gen, quality: u8) -> u8 {
    let x = g.rng.below(100);
    let r = if x < 60 {
        i32::from(quality)
    } else if x < 80 {
        i32::from(quality) + if g.rng.chance(1, 2) { 1 } else { -1 }
    } else {
        g.rng.range(1, 5) as i32
    };
    r.clamp(1, 5) as u8
}

fn review(g: &mut Gen, bi: usize, pi: usize) {
    let (buyer, verified) = g.bulk.buyers[bi];
    let prod = g.bulk.products[pi].clone();
    let stars = rating(g, prod.quality);
    let mut text = (*g.rng.pick(REVIEW_TEXT[usize::from(stars) - 1])).to_owned();
    if g.rng.chance(1, 2) {
        text.push_str(&format!(" Used it for {} weeks.", g.rng.range(1, 30)));
    }
    let title = if g.rng.chance(1, 2) { Some(REVIEW_TITLES[usize::from(stars) - 1]) } else { None };
    let mut listing = None;
    let mut receipt = None;
    if verified && !prod.listings.is_empty() && g.rng.chance(55, 100) {
        let l = g.rng.pick(&prod.listings).clone();
        let root_ts = g.listing_info(&l).root_ts;
        let t = g.now();
        let purchased = (t - g.rng.range(1, 20) * DAY).max(root_ts);
        receipt = Some(g.receipt(&l, buyer, purchased));
        listing = Some(l);
    }
    let r = g.review_key("", buyer, &prod.key, stars, title, &text, listing.as_deref(), receipt.as_ref(), None);
    if g.rng.chance(1, 20) {
        let ts = (g.now() + g.rng.range(1, 30) * DAY).min(BASE_TS + 195 * DAY);
        let key = prod.key.clone();
        let new = if stars > 1 { stars - 1 } else { 2 };
        let root = r.id();
        g.at_ts(ts, move |g| {
            g.review_key("", buyer, &key, new, Some("Update"), "Update after more use.", listing.as_deref(), receipt.as_ref(), Some(root));
        });
    }
    if g.rng.chance(1, 200) {
        let ts = BASE_TS + 196 * DAY + g.rng.below(3 * DAY);
        let root: RecordId = r.id();
        g.at_ts(ts, move |g| {
            if let Some(f) = random_verified_buyer(g) {
                g.flag(f, root, "spam", None);
            }
        });
    }
}
