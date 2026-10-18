//! Search (docs/views.md section 11).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use perch_core::kinds::{Body, names};
use perch_core::ProductKey;
use perch_store::Store;

use crate::engine::Views;
use crate::model::{Price, SearchHit, Status};
use crate::num::{Num, Ratio, mean};

/// Search sort order (ARCHITECTURE 7.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    /// `score` descending.
    #[default]
    Relevance,
    /// Lowest price first, within the sort currency first.
    Price,
    /// Most verified-identity reviews first.
    RatingCount,
    /// Highest smoothed verified-identity rating first.
    RatingMean,
}

impl Sort {
    /// Parses `relevance`, `price`, `rating_count`, or `rating_mean`.
    pub fn parse(s: &str) -> Option<Sort> {
        Some(match s {
            "relevance" => Sort::Relevance,
            "price" => Sort::Price,
            "rating_count" => Sort::RatingCount,
            "rating_mean" => Sort::RatingMean,
            _ => return None,
        })
    }
}

/// Search options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOptions {
    /// Sort order.
    pub sort: Sort,
    /// Preferred currency for `min_price` and price sorting.
    pub currency: Option<String>,
    /// Maximum hits (at most [`MAX_LIMIT`]).
    pub limit: usize,
}

/// Default number of hits.
pub const DEFAULT_LIMIT: usize = 100;
/// Largest allowed `limit`.
pub const MAX_LIMIT: usize = 1_000;
/// Query tokens used.
pub const MAX_QUERY_TOKENS: usize = 16;

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions { sort: Sort::Relevance, currency: None, limit: DEFAULT_LIMIT }
    }
}

/// Tokens of `s` (11.1): maximal runs of alphanumeric characters,
/// lowercased with Unicode default case conversion.
pub fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            cur.extend(c.to_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Query tokens: deduplicated (first occurrence kept), at most 16.
pub fn query_tokens(q: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for t in tokens(q) {
        if seen.insert(t.clone()) {
            out.push(t);
            if out.len() == MAX_QUERY_TOKENS {
                break;
            }
        }
    }
    out
}

/// One field's match of a query token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Match {
    points: i64,
    field: usize,
    exact: bool,
}

const FIELDS: [(&str, i64, i64); 5] = [("gtin", 100, 0), ("name", 30, 15), ("mpn", 25, 10), ("brand", 20, 10), ("category", 10, 5)];

fn better(a: &Match, b: &Match) -> bool {
    (a.points, std::cmp::Reverse(a.field), a.exact) > (b.points, std::cmp::Reverse(b.field), b.exact)
}

fn match_token(t: &str, gtin: Option<&str>, fields: &[Vec<String>; 4]) -> Option<Match> {
    let mut best: Option<Match> = None;
    let mut consider = |m: Match| {
        if best.as_ref().is_none_or(|b| better(&m, b)) {
            best = Some(m);
        }
    };
    if let Some(g) = gtin
        && (8..=14).contains(&t.len())
        && t.bytes().all(|b| b.is_ascii_digit())
        && format!("{t:0>14}") == g
    {
        consider(Match { points: 100, field: 0, exact: true });
    }
    let long_enough = t.chars().count() >= 2;
    for (i, toks) in fields.iter().enumerate() {
        let (_, exact, prefix) = FIELDS[i + 1];
        if toks.iter().any(|x| x == t) {
            consider(Match { points: exact, field: i + 1, exact: true });
        } else if long_enough && toks.iter().any(|x| x.len() > t.len() && x.starts_with(t)) {
            consider(Match { points: prefix, field: i + 1, exact: false });
        }
    }
    best
}

fn signed(n: Num) -> String {
    if n.0 >= 0 { format!("+{}", n.text()) } else { n.text() }
}

struct Hit {
    hit: SearchHit,
    smoothed: Ratio,
    visible: bool,
}

/// A product that matched every query token, before rating and price.
struct TextHit {
    key: ProductKey,
    points: i64,
    why: Vec<String>,
    text: crate::views::ProductText,
    status: Status,
}

impl<S: Store + ?Sized> Views<'_, S> {
    /// Product chains by product key, from every product record (10.2).
    pub(crate) fn product_chain_index(&self) -> BTreeMap<String, Vec<std::sync::Arc<crate::engine::Chain>>> {
        let mut roots = BTreeSet::new();
        for id in self.store.by_kind(names::PRODUCT) {
            roots.insert(self.root_of(&id));
        }
        let mut out: BTreeMap<String, Vec<_>> = BTreeMap::new();
        for r in roots {
            let Some(c) = self.chain(&r) else { continue };
            if let Some(k) = self.chain_product_key(&c.base) {
                out.entry(k).or_default().push(c);
            }
        }
        out
    }

    /// Product keys of product chains with a reference version.
    pub(crate) fn product_chain_keys(&self) -> BTreeSet<String> {
        self.product_chain_index().into_keys().collect()
    }

    /// Search (11).
    pub fn search(&self, q: &str, opts: &SearchOptions) -> Vec<SearchHit> {
        let qt = query_tokens(q);
        let limit = opts.limit.min(MAX_LIMIT);
        let mut index = self.product_chain_index();
        for t in &qt {
            if (8..=14).contains(&t.len())
                && t.bytes().all(|b| b.is_ascii_digit())
                && let Ok(k) = ProductKey::from_gtin(t)
                && !index.contains_key(k.as_str())
                && self.product_exists(k.as_str())
            {
                index.insert(k.as_str().to_owned(), Vec::new());
            }
        }
        // Pass 1: text matching over product chains only.
        let mut text_hits: Vec<TextHit> = Vec::new();
        for (key, chains) in &index {
            if let Some(h) = self.text_match(key, chains, &qt) {
                text_hits.push(h);
            }
        }
        // Pass 2: ratings and prices. For relevance, hits that cannot reach
        // the top `limit` are skipped: rating and offer add less than 25 points.
        let mut hits: Vec<Hit> = Vec::new();
        if opts.sort == Sort::Relevance {
            text_hits.sort_by(|a, b| {
                (b.status == Status::Visible).cmp(&(a.status == Status::Visible)).then(b.points.cmp(&a.points)).then(a.key.cmp(&b.key))
            });
            let mut groups: Vec<Vec<TextHit>> = vec![Vec::new(), Vec::new()];
            for h in text_hits {
                groups[usize::from(h.status != Status::Visible)].push(h);
            }
            for group in groups {
                let need = limit.saturating_sub(hits.len());
                if need == 0 {
                    break;
                }
                let mut scored: Vec<Hit> = Vec::new();
                let mut best: Vec<Num> = Vec::new();
                for th in group {
                    if best.len() >= need && Num(th.points * 100 + 2500) <= best[need - 1] {
                        break;
                    }
                    let h = self.finish_hit(th, opts);
                    let pos = best.partition_point(|s| *s >= h.hit.score);
                    best.insert(pos, h.hit.score);
                    scored.push(h);
                }
                scored.sort_by(|a, b| b.hit.score.cmp(&a.hit.score).then_with(|| a.hit.key.cmp(&b.hit.key)));
                scored.truncate(need);
                hits.extend(scored);
            }
        } else {
            hits = text_hits.into_iter().map(|th| self.finish_hit(th, opts)).collect();
            let sort_currency = opts.currency.clone().or_else(|| {
                let mut n: BTreeMap<&str, u64> = BTreeMap::new();
                for h in &hits {
                    if let Some(p) = &h.hit.min_price {
                        *n.entry(p.currency.as_str()).or_default() += 1;
                    }
                }
                n.into_iter().min_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0))).map(|(c, _)| c.to_owned())
            });
            hits.sort_by(|a, b| {
                b.visible
                    .cmp(&a.visible)
                    .then_with(|| match opts.sort {
                        Sort::Relevance => Ordering::Equal,
                        Sort::Price => price_key(&a.hit.min_price, sort_currency.as_deref())
                            .cmp(&price_key(&b.hit.min_price, sort_currency.as_deref())),
                        Sort::RatingCount => {
                            b.hit.rating_count.cmp(&a.hit.rating_count).then(b.smoothed.cmp(&a.smoothed))
                        }
                        Sort::RatingMean => {
                            b.smoothed.cmp(&a.smoothed).then(b.hit.rating_count.cmp(&a.hit.rating_count))
                        }
                    })
                    .then_with(|| a.hit.key.cmp(&b.hit.key))
            });
            hits.truncate(limit);
        }
        hits.into_iter()
            .map(|mut h| {
                let r = self.ratings(h.hit.key.as_str(), true);
                h.hit.verified_purchase_count = r.verified_purchase.count;
                h.hit.verified_purchase_mean = r.verified_purchase.mean;
                h.hit
            })
            .collect()
    }

    fn text_match(&self, key: &str, chains: &[std::sync::Arc<crate::engine::Chain>], qt: &[String]) -> Option<TextHit> {
        let pkey = ProductKey::parse(key).ok()?;
        let ps = self.product_sources_of(chains);
        if ps.all.is_empty() && !self.product_exists(key) {
            return None;
        }
        let text = self.product_text(&ps);
        // Every candidate value counts, not only the displayed one (7.8).
        let [names, brands, categories] = text.candidates();
        let toks = |vals: &[&str]| -> Vec<String> {
            let mut t: Vec<String> = vals.iter().flat_map(|v| tokens(v)).collect();
            t.sort();
            t.dedup();
            t
        };
        let fields = [toks(&names), text.mpn.as_deref().map(tokens).unwrap_or_default(), toks(&brands), toks(&categories)];
        let mut why = Vec::new();
        let mut points: i64 = 0;
        for t in qt {
            let m = match_token(t, pkey.gtin(), &fields)?;
            points += m.points;
            why.push(format!("{} {} \"{}\" +{}", FIELDS[m.field].0, if m.exact { "exact" } else { "prefix" }, t, m.points));
        }
        Some(TextHit { key: pkey, points, why, text, status: ps.status })
    }

    fn finish_hit(&self, th: TextHit, opts: &SearchOptions) -> Hit {
        let key = th.key.as_str();
        let mut why = th.why;
        // Rating component: verified-identity reviews only (7.8).
        let set = self.review_set(key);
        let (mut n, mut sum) = (0u64, 0u64);
        for e in &set.evals {
            if e.excluded.is_none()
                && self.verified(&e.chain.author())
                && let Some(Body::Review(r)) = e.chain.latest_body()
            {
                n += 1;
                sum += u64::from(r.rating);
            }
        }
        let smoothed = Ratio::new(15 + i128::from(sum), i128::from(n) + 5);
        let rating_points = Num::round(Ratio::new(10 * (i128::from(sum) - 3 * i128::from(n)), i128::from(n) + 5));
        let smoothed_num = Num::round(smoothed);
        why.push(format!("rating {} from {} verified-identity reviews {}", smoothed_num.text(), n, signed(rating_points)));
        let min_price: Option<Price> = self.min_price(key, opts.currency.as_deref());
        let offer = if min_price.is_some() { 5 } else { 0 };
        why.push(if offer > 0 { "has price +5".to_owned() } else { "no price +0".to_owned() });
        let score = Num(th.points * 100 + rating_points.0 + offer * 100);
        Hit {
            visible: th.status == Status::Visible,
            smoothed,
            hit: SearchHit {
                key: th.key,
                name: th.text.name.value,
                brand: th.text.brand.value,
                category: th.text.category.map(|c| c.value),
                score,
                why,
                moderation_status: th.status,
                rating_mean: mean(sum, n),
                rating_count: n,
                rating_smoothed: smoothed_num,
                verified_purchase_count: 0,
                verified_purchase_mean: None,
                min_price,
            },
        }
    }
}

fn price_key(p: &Option<Price>, c: Option<&str>) -> (u8, String, u64) {
    match p {
        Some(p) if Some(p.currency.as_str()) == c => (0, String::new(), p.amount),
        Some(p) => (1, p.currency.clone(), p.amount),
        None => (2, String::new(), 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer() {
        assert_eq!(tokens("Alder Kettle 1.7 L"), ["alder", "kettle", "1", "7", "l"]);
        assert_eq!(tokens("Straße—ÉCLAIR x"), ["straße", "éclair", "x"]);
        assert_eq!(query_tokens("a a b"), ["a", "b"]);
    }
}
