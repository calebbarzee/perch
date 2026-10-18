// Mock views: a small JavaScript model of perch-views that returns the
// ARCHITECTURE 7.8 shapes. It is used only when the wasm client is missing or
// the URL has ?mock=1. Results are plausible, not authoritative: the wasm
// client and the native node run the real rules in Rust.

import { productKeyFor } from './mock-data.js';
import { verifyRecordMock, receiptMessage, mockSign } from './mock-crypto.js';
import { sha256Hex } from './sha256.js';

const CMP = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const CONTROL_KINDS = new Set(['attest.identity', 'attest.domain', 'attest.revoke', 'retract', 'retire']);
const HIDEABLE = new Set(['listing', 'review', 'product', 'profile']);

// Exact half-to-even rounding of num/den to 2 decimals (ARCHITECTURE 7.7).
export function meanOf(sum, count) {
  if (!count) return null;
  const scaled = sum * 100;
  let q = Math.floor(scaled / count);
  const r = scaled - q * count;
  if (r * 2 > count || (r * 2 === count && q % 2 === 1)) q += 1;
  return q / 100;
}

function hostOf(url) {
  try {
    const u = new URL(url);
    return u.protocol === 'https:' ? u.hostname : null;
  } catch {
    return null;
  }
}

export class MockStore {
  constructor() {
    this.byId = new Map(); // id -> record (wire form)
    this.logs = new Map(); // author -> array indexed by seq-1 of ids
    this.evidence = new Map(); // author -> [a, b]
    this.forkSeq = new Map(); // author -> n
    this.held = new Map(); // author -> Map(seq -> record)
    this.rejectedCount = 0;
  }

  head(author) {
    const log = this.logs.get(author);
    if (!log || !log.length) return null;
    const seq = log.length;
    return { seq, id: log[seq - 1] };
  }

  heads() {
    const out = {};
    for (const a of this.logs.keys()) {
      const hd = this.head(a);
      if (hd) out[a] = { ...hd, forked: this.forkSeq.has(a) };
    }
    return out;
  }

  // Returns 'inserted' | 'duplicate' | 'fork' | 'gap' | 'rejected'.
  insert(rec) {
    const check = verifyRecordMock(rec);
    if (!check.valid) {
      this.rejectedCount++;
      return { outcome: 'rejected', error: check.error, id: check.id };
    }
    const id = check.id;
    const existing = this.byId.get(id);
    if (existing) {
      if (!('body' in existing) && 'body' in rec) {
        this.byId.set(id, rec);
        return { outcome: 'inserted', id };
      }
      return { outcome: 'duplicate', id };
    }
    const author = rec.author;
    if (this.forkSeq.has(author) && rec.seq >= this.forkSeq.get(author)) {
      return { outcome: 'duplicate', id };
    }
    const log = this.logs.get(author) || [];
    if (rec.seq <= log.length) {
      // Same seq, different id: fork evidence.
      const otherId = log[rec.seq - 1];
      this.recordFork(author, rec.seq, this.byId.get(otherId), rec, id);
      return { outcome: 'fork', id };
    }
    if (rec.seq > log.length + 1) {
      if (!this.held.has(author)) this.held.set(author, new Map());
      this.held.get(author).set(rec.seq, rec);
      return { outcome: 'gap', id };
    }
    if (log.length && rec.prev !== log[log.length - 1]) {
      this.recordFork(author, rec.seq - 1, this.byId.get(log[log.length - 1]), rec, id);
      return { outcome: 'fork', id };
    }
    if (log.length && rec.ts < this.byId.get(log[log.length - 1]).ts) {
      this.rejectedCount++;
      return { outcome: 'rejected', id, error: 'ts is lower than the previous record in the log.' };
    }
    log.push(id);
    this.logs.set(author, log);
    this.byId.set(id, rec);
    // Release held records that now connect.
    const held = this.held.get(author);
    if (held && held.has(log.length + 1)) {
      const next = held.get(log.length + 1);
      held.delete(log.length + 1);
      this.insert(next);
    }
    return { outcome: 'inserted', id };
  }

  recordFork(author, n, a, b, bId) {
    const prev = this.forkSeq.get(author);
    if (prev === undefined || n < prev) {
      this.forkSeq.set(author, n);
      this.evidence.set(author, [a, b]);
      this.byId.set(bId, b);
      const log = this.logs.get(author) || [];
      for (const dropped of log.slice(n - 1)) {
        if (dropped !== a && dropped !== bId) this.byId.delete(dropped);
      }
      this.logs.set(author, log.slice(0, n - 1));
    }
  }

  // Records in log order (evidence pairs last), for export and views.
  all() {
    const out = [];
    for (const log of this.logs.values()) for (const id of log) out.push(this.byId.get(id));
    for (const pair of this.evidence.values()) for (const r of pair) if (r) out.push(r);
    return out;
  }

  idOf(rec) {
    return verifyRecordMock(rec).id;
  }
}

// Computes every view from the store and policy at once. Cached per
// (store version, policy JSON) by the mock client.
export function computeState(store, policy) {
  const now = policy.now ?? Date.now();
  const tolerance = policy.future_ts_tolerance_ms ?? 600000;

  const recs = new Map(); // id -> { id, r }
  const logEntries = [];
  for (const [author, log] of store.logs) {
    log.forEach((id, i) => logEntries.push({ id, r: store.byId.get(id), author, seq: i + 1 }));
  }
  for (const e of logEntries) recs.set(e.id, e);

  const attestorTrust = new Map();
  for (const a of policy.trusted_attestors || []) attestorTrust.set(a.author, a);

  // Revokes and retire cut-offs.
  const revoked = new Map(); // subject -> { attestor, valid_through_seq }
  const retiredAt = new Map(); // author -> seq of retire
  const subjectsByAttestor = new Map();
  for (const e of logEntries) {
    if (!e.r.body) continue;
    const t = attestorTrust.get(e.author);
    if ((e.r.kind === 'attest.identity' || e.r.kind === 'attest.domain') && t) {
      if (!subjectsByAttestor.has(e.author)) subjectsByAttestor.set(e.author, new Set());
      subjectsByAttestor.get(e.author).add(e.r.body.subject);
    }
  }
  for (const e of logEntries) {
    if (!e.r.body) continue;
    if (e.r.kind === 'attest.revoke') {
      const t = attestorTrust.get(e.author);
      if (!t || !(t.kinds || []).includes('attest.revoke')) continue;
      if (t.not_after_seq != null && e.seq > t.not_after_seq) continue;
      if (!subjectsByAttestor.get(e.author)?.has(e.r.body.subject)) continue;
      const prev = revoked.get(e.r.body.subject);
      if (!prev || e.r.body.valid_through_seq < prev.valid_through_seq) {
        revoked.set(e.r.body.subject, { attestor: e.author, valid_through_seq: e.r.body.valid_through_seq });
      }
    }
    if (e.r.kind === 'retire' && !retiredAt.has(e.author)) retiredAt.set(e.author, e.seq);
  }

  const active = (e) => {
    if (e.r.ts > now + tolerance) return false;
    const rv = revoked.get(e.author);
    if (rv && e.seq > rv.valid_through_seq) return false;
    const rt = retiredAt.get(e.author);
    if (rt !== undefined && e.seq > rt) return false;
    return true;
  };
  const revokedAfterCutoff = (e) => {
    const rv = revoked.get(e.author);
    return rv && e.seq > rv.valid_through_seq;
  };

  // Chains.
  const rootOf = new Map();
  const root = (id) => {
    if (rootOf.has(id)) return rootOf.get(id);
    const e = recs.get(id);
    let out = id;
    if (e && e.r.replaces) {
      const parent = recs.get(e.r.replaces);
      if (parent && parent.author === e.author && parent.seq < e.seq && parent.r.kind === e.r.kind) out = root(e.r.replaces);
    }
    rootOf.set(id, out);
    return out;
  };
  const chains = new Map(); // root -> [entries by seq]
  for (const e of logEntries) {
    if (!active(e) && !revokedAfterCutoff(e)) continue;
    const rt = root(e.id);
    if (!chains.has(rt)) chains.set(rt, []);
    chains.get(rt).push(e);
  }
  const latestOf = (rt) => {
    const list = (chains.get(rt) || []).filter(active);
    return list.length ? list[list.length - 1] : null;
  };

  // Retractions.
  const retracted = new Map(); // root -> retract entry
  for (const e of logEntries) {
    if (!active(e) || e.r.kind !== 'retract' || !e.r.body) continue;
    const target = recs.get(e.r.body.target);
    if (!target || target.author !== e.author) continue;
    retracted.set(root(target.id), e);
  }

  // Attestations.
  const identity = new Map(); // subject -> { level, country, attestor, expires, person }
  const domains = new Map(); // subject -> [{ domain, attestor, expires }]
  for (const [rt] of chains) {
    const e = latestOf(rt);
    if (!e || !e.r.body || retracted.has(rt)) continue;
    const t = attestorTrust.get(e.author);
    if (!t || !(t.kinds || []).includes(e.r.kind)) continue;
    if (t.not_after_seq != null && e.seq > t.not_after_seq) continue;
    const b = e.r.body;
    if (!(b.expires > now)) continue;
    if (e.r.kind === 'attest.identity') {
      const cand = { level: b.level, country: b.country ?? null, attestor: e.author, expires: b.expires, person: b.person };
      const prev = identity.get(b.subject);
      if (!prev || CMP(cand.attestor + cand.person, prev.attestor + prev.person) < 0) identity.set(b.subject, cand);
    } else if (e.r.kind === 'attest.domain') {
      if (!domains.has(b.subject)) domains.set(b.subject, []);
      domains.get(b.subject).push({ domain: b.domain, attestor: e.author, expires: b.expires });
    }
  }
  for (const list of domains.values()) list.sort((a, b) => CMP(a.domain, b.domain) || CMP(a.attestor, b.attestor));
  const verified = (author) => identity.has(author);
  const personKey = (author) => {
    const v = identity.get(author);
    return v ? v.attestor + ':' + v.person : 'key:' + author;
  };

  // Profiles.
  const profiles = new Map(); // author -> latest profile body
  for (const [rt] of chains) {
    const e = latestOf(rt);
    if (!e || e.r.kind !== 'profile' || !e.r.body || retracted.has(rt)) continue;
    const prev = profiles.get(e.author);
    if (!prev || prev.seq < e.seq) profiles.set(e.author, { seq: e.seq, body: e.r.body, root: rt });
  }
  const nameOf = (a) => profiles.get(a)?.body.name ?? null;

  const authorStatus = (a) =>
    store.forkSeq.has(a) ? 'forked' : revoked.has(a) ? 'revoked' : retiredAt.has(a) ? 'retired' : 'ok';

  // Redactions.
  const redactNotices = new Map(); // target id -> entry
  for (const e of logEntries) {
    if (!active(e) || e.r.kind !== 'redact' || !e.r.body) continue;
    if (!redactNotices.has(e.r.body.target)) redactNotices.set(e.r.body.target, e);
  }
  const redactionFor = (e) => {
    if (!e || e.r.body) return null;
    const n = redactNotices.get(e.id);
    if (!n) return { record: null, by: null, reason: null, note: null };
    return { record: n.id, by: n.author, reason: n.r.body.reason, note: n.r.body.note ?? null };
  };

  // Flags.
  const flagsOn = new Map(); // root -> Map(flagger -> entry)
  for (const [rt] of chains) {
    const e = latestOf(rt);
    if (!e || e.r.kind !== 'flag' || !e.r.body || retracted.has(rt)) continue;
    if (store.forkSeq.has(e.author)) continue;
    const target = recs.get(e.r.body.target);
    if (!target) continue;
    const troot = root(target.id);
    if (!flagsOn.has(troot)) flagsOn.set(troot, new Map());
    flagsOn.get(troot).set(e.author, e);
  }
  const flagSummary = (troot, targetAuthor) => {
    const m = flagsOn.get(troot) || new Map();
    const reasons = {};
    const persons = new Set();
    let count = 0;
    for (const [flagger, e] of m) {
      if (flagger === targetAuthor) continue;
      count++;
      reasons[e.r.body.reason] = (reasons[e.r.body.reason] || 0) + 1;
      if (policy.flags_require_verified && !verified(flagger)) continue;
      persons.add(personKey(flagger));
    }
    return { count, counted: persons.size, reasons, persons };
  };
  const thresholdFor = (kind) => policy.flag_thresholds?.[kind] ?? policy.flag_threshold ?? 3;

  // Appeals.
  const appeals = new Map(); // target root -> Appeal
  for (const [rt] of chains) {
    const e = latestOf(rt);
    if (!e || e.r.kind !== 'appeal' || !e.r.body || retracted.has(rt)) continue;
    const target = recs.get(e.r.body.target);
    if (!target || target.author !== e.author) continue;
    appeals.set(root(target.id), { record: e.id, note: e.r.body.note, ts: e.r.ts });
  }

  const statusOf = (rt, e, kind) => {
    if (retracted.has(rt)) return 'retracted';
    if (!e.r.body) return 'unavailable';
    if (HIDEABLE.has(kind)) {
      const f = flagSummary(rt, e.author);
      if (f.counted >= thresholdFor(kind)) return 'hidden';
    }
    return 'visible';
  };

  // Products.
  const products = new Map(); // key -> [{ e, rt }]
  for (const [rt] of chains) {
    const e = latestOf(rt);
    if (!e || e.r.kind !== 'product' || !e.r.body || retracted.has(rt)) continue;
    const key = productKeyFor(e.r.body, rt);
    if (!products.has(key)) products.set(key, []);
    products.get(key).push({ e, rt });
  }

  // Listings.
  const observationsByListing = new Map();
  for (const e of logEntries) {
    if (!active(e) || e.r.kind !== 'observation' || !e.r.body || !e.r.body.listing) continue;
    const t = recs.get(e.r.body.listing);
    if (!t) continue;
    const lr = root(t.id);
    if (!observationsByListing.has(lr)) observationsByListing.set(lr, []);
    observationsByListing.get(lr).push(e);
  }
  const listingViews = new Map();
  const listingsBySeller = new Map();
  for (const [rt, chain] of chains) {
    const e = latestOf(rt);
    if (!e || e.r.kind !== 'listing') continue;
    const first = chain[0];
    const b = e.r.body || {};
    const status = statusOf(rt, e, 'listing');
    const f = flagSummary(rt, e.author);
    const host = hostOf(b.url);
    const dom = (domains.get(e.author) || []).find((d) => host && (host === d.domain || host.endsWith('.' + d.domain))) || null;
    const history = [];
    for (const v of chain.filter(active)) {
      if (v.r.body) history.push({ ts: v.r.ts, amount: v.r.body.price.amount, currency: v.r.body.price.currency, source: 'seller', record: v.id });
    }
    for (const o of observationsByListing.get(rt) || []) {
      history.push({ ts: o.r.body.observed_at, amount: o.r.body.price.amount, currency: o.r.body.price.currency, source: 'observation', record: o.id });
    }
    history.sort((x, y) => x.ts - y.ts || CMP(x.record, y.record));
    const view = {
      id: rt,
      latest: e.id,
      seller: e.author,
      seller_name: nameOf(e.author),
      seller_verified: verified(e.author),
      seller_restricted: false,
      seller_status: authorStatus(e.author),
      domain_verified: Boolean(dom),
      domain: dom ? { ...dom } : null,
      product: b.product ?? (first.r.body ? first.r.body.product : null),
      title: b.title ?? null,
      url: b.url ?? null,
      price: b.price ? { ...b.price } : null,
      availability: b.availability ?? null,
      condition: b.condition ?? null,
      updated_ts: e.r.ts,
      status,
      flags: { count: f.count, counted: f.counted, reasons: f.reasons },
      appealed: appeals.has(rt),
      appeal: appeals.get(rt) ?? null,
      redaction: redactionFor(e),
      price_history: history,
      _flaggers: f.persons,
    };
    listingViews.set(rt, view);
    const sk = personKey(e.author);
    if (!listingsBySeller.has(sk)) listingsBySeller.set(sk, []);
    listingsBySeller.get(sk).push(view);
  }
  // Restriction per person key.
  const restrictedPersons = new Set();
  for (const [sk, list] of listingsBySeller) {
    const hidden = list.filter((l) => l.status === 'hidden');
    const flaggers = new Set();
    for (const l of hidden) for (const p of l._flaggers) flaggers.add(p);
    if (hidden.length >= (policy.seller_hidden_listing_limit ?? 3) && flaggers.size >= (policy.seller_min_distinct_flaggers ?? 9)) {
      restrictedPersons.add(sk);
    }
  }
  for (const author of store.forkSeq.keys()) if (identity.has(author) && !revoked.has(author)) restrictedPersons.add(personKey(author));
  const isRestricted = (a) => restrictedPersons.has(personKey(a));
  for (const l of listingViews.values()) {
    l.seller_restricted = isRestricted(l.seller);
    delete l._flaggers;
  }

  // Reviews.
  const reviewsByProduct = new Map();
  for (const [rt, chain] of chains) {
    if (chain[0].r.kind !== 'review') continue;
    let e = latestOf(rt);
    const afterCutoff = !e;
    if (!e) e = chain[chain.length - 1];
    if (!e || (!active(e) && !revokedAfterCutoff(e))) continue;
    const b = e.r.body;
    const product = b ? b.product : chain.find((c) => c.r.body)?.r.body.product;
    if (!product) continue;
    const f = flagSummary(rt, e.author);
    const listingRoot = b && b.listing && recs.get(b.listing) ? root(b.listing) : null;
    const view = {
      id: rt,
      latest: e.id,
      author: e.author,
      author_name: nameOf(e.author),
      author_verified: verified(e.author),
      author_status: authorStatus(e.author),
      verified_purchase: false,
      purchase: 'none',
      counted: false,
      excluded_reason: null,
      rating: b ? b.rating : null,
      title: b ? b.title ?? null : null,
      text: b ? b.text : null,
      ts: e.r.ts,
      listing: listingRoot,
      status: statusOf(rt, e, 'review'),
      flags: { count: f.count, counted: f.counted, reasons: f.reasons },
      appeal: appeals.get(rt) ?? null,
      redaction: redactionFor(e),
      _seq: e.seq,
      _afterCutoff: afterCutoff || revokedAfterCutoff(e),
      _receipt: b ? b.receipt : null,
    };
    if (!reviewsByProduct.has(product)) reviewsByProduct.set(product, []);
    reviewsByProduct.get(product).push(view);
  }

  const sellersOf = (key) => {
    const s = new Set();
    for (const l of listingViews.values()) if (l.product === key) s.add(l.seller);
    return s;
  };

  const purchaseOf = (rv) => {
    const rc = rv._receipt;
    if (!rc) return 'none';
    const lroot = recs.get(rc.listing) ? root(rc.listing) : null;
    const lv = lroot ? listingViews.get(lroot) : null;
    if (!lv || mockSign(lv.seller, receiptMessage(rc)) !== rc.sig) return 'receipt_invalid';
    if (rc.buyer !== rv.author) return 'receipt_invalid';
    if (lv.product !== rv._product) return 'wrong_product';
    if (!verified(lv.seller)) return 'seller_unverified';
    if (policy.verified_purchase_requires_verified_buyer && !verified(rv.author)) return 'buyer_unverified';
    if (rv.author === lv.seller) return 'self';
    return 'verified';
  };

  const productViews = new Map();
  const allKeys = new Set([...products.keys()]);
  for (const k of allKeys) {
    const prods = products.get(k) || [];
    const listings = [...listingViews.values()].filter((l) => l.product === k);
    listings.sort((a, b) => CMP(a.status === 'visible' ? 0 : 1, b.status === 'visible' ? 0 : 1) || (a.price?.amount ?? 0) - (b.price?.amount ?? 0) || CMP(a.id, b.id));
    const sellers = sellersOf(k);
    const reviews = reviewsByProduct.get(k) || [];
    // Latest per (author, product).
    const latestByAuthor = new Map();
    for (const rv of reviews) {
      rv._product = k;
      if (rv._afterCutoff) continue;
      const prev = latestByAuthor.get(rv.author);
      if (!prev || prev._seq < rv._seq) latestByAuthor.set(rv.author, rv);
    }
    const excluded = { hidden: 0, unavailable: 0, retracted: 0, seller: 0, forked: 0, revoked: 0, superseded: 0 };
    for (const rv of reviews) {
      rv.purchase = rv.status === 'unavailable' ? 'none' : purchaseOf(rv);
      rv.verified_purchase = rv.purchase === 'verified';
      let reason = null;
      if (rv.status === 'retracted') reason = 'retracted';
      else if (rv.status === 'unavailable') reason = 'unavailable';
      else if (rv._afterCutoff) reason = 'revoked';
      else if (rv.author_status === 'forked') reason = 'forked';
      else if (sellers.has(rv.author)) reason = 'seller';
      else if (latestByAuthor.get(rv.author) !== rv) reason = 'superseded';
      else if (rv.status === 'hidden') reason = 'hidden';
      rv.excluded_reason = reason;
      rv.counted = reason === null;
      if (reason) excluded[reason]++;
    }
    const summarize = (list) => {
      const histogram = [0, 0, 0, 0, 0];
      let sum = 0;
      for (const rv of list) {
        histogram[rv.rating - 1]++;
        sum += rv.rating;
      }
      return { count: list.length, mean: meanOf(sum, list.length), histogram };
    };
    const counted = reviews.filter((r) => r.counted);
    const vi = new Map();
    for (const rv of counted) if (rv.author_verified && !vi.has(personKey(rv.author))) vi.set(personKey(rv.author), rv);
    const all = summarize(counted);
    const ratings = {
      ...all,
      verified_purchase: summarize(counted.filter((r) => r.verified_purchase)),
      verified_identity: summarize([...vi.values()]),
      excluded,
    };
    reviews.sort((a, b) => CMP(a.counted ? 0 : 1, b.counted ? 0 : 1) || b.ts - a.ts || CMP(a.id, b.id));

    const priceRange = {};
    for (const l of listings) {
      if (l.status !== 'visible' || !l.price) continue;
      const c = l.price.currency;
      priceRange[c] ??= { amounts: [], excluded_unverified_domain: 0 };
      if (policy.price_requires_domain_verified && !l.domain_verified) {
        priceRange[c].excluded_unverified_domain++;
        continue;
      }
      priceRange[c].amounts.push(l.price.amount);
    }
    const price_range = {};
    for (const [c, v] of Object.entries(priceRange)) {
      const a = v.amounts.sort((x, y) => x - y);
      price_range[c] = {
        min: a.length ? a[0] : null,
        max: a.length ? a[a.length - 1] : null,
        median: a.length ? a[Math.floor((a.length - 1) / 2)] : null,
        count: a.length,
        excluded_unverified_domain: v.excluded_unverified_domain,
      };
    }

    const fact = (field) => {
      const groups = new Map();
      for (const { e } of prods) {
        const value = e.r.body[field];
        if (value === undefined) continue;
        if (!groups.has(value)) groups.set(value, []);
        groups.get(value).push({ author: e.author, record: e.id, verified: verified(e.author) });
      }
      if (!groups.size) return null;
      const ranked = [...groups.entries()].sort(
        (a, b) =>
          b[1].filter((s) => s.verified).length - a[1].filter((s) => s.verified).length ||
          b[1].length - a[1].length ||
          CMP(a[0], b[0]),
      );
      const [[value, sources], ...rest] = ranked;
      return { value, sources, conflicts: rest.map(([v, s]) => ({ value: v, sources: s })) };
    };
    const attributes = {};
    const attrNames = new Set();
    for (const { e } of prods) for (const n of Object.keys(e.r.body.attributes || {})) attrNames.add(n);
    for (const n of [...attrNames].sort()) {
      const groups = new Map();
      for (const { e } of prods) {
        const v = e.r.body.attributes?.[n];
        if (v === undefined) continue;
        if (!groups.has(v)) groups.set(v, []);
        groups.get(v).push({ author: e.author, record: e.id, verified: verified(e.author) });
      }
      const ranked = [...groups.entries()].sort((a, b) => b[1].length - a[1].length || CMP(a[0], b[0]));
      attributes[n] = { value: ranked[0][0], sources: ranked[0][1], conflicts: ranked.slice(1).map(([v, s]) => ({ value: v, sources: s })) };
    }
    const images = [];
    const seenImg = new Set();
    for (const { e } of prods) {
      for (const img of e.r.body.images || []) {
        if (seenImg.has(img.url)) continue;
        seenImg.add(img.url);
        images.push({ url: img.url, sha256: img.sha256 ?? null, author_verified: verified(e.author) });
      }
    }
    const firstBody = prods[0]?.e.r.body || {};
    const view = {
      key: k,
      name: fact('name'),
      brand: fact('brand'),
      category: fact('category'),
      description: fact('description'),
      gtin: k.startsWith('gtin:') ? k.slice(5) : null,
      mpn: firstBody.mpn ?? null,
      attributes,
      images,
      listings,
      reviews,
      ratings,
      price_range,
    };
    for (const rv of reviews) {
      delete rv._seq;
      delete rv._afterCutoff;
      delete rv._receipt;
      delete rv._product;
    }
    productViews.set(k, view);
  }

  // Authors.
  const authorViews = new Map();
  const authorIds = new Set([...store.logs.keys(), ...store.forkSeq.keys()]);
  for (const a of authorIds) {
    const p = profiles.get(a)?.body;
    const id = identity.get(a);
    const sellerListings = [...listingViews.values()].filter((l) => l.seller === a);
    authorViews.set(a, {
      id: a,
      name: p?.name ?? null,
      role: p?.role ?? null,
      url: p?.url ?? null,
      about: p?.about ?? null,
      verified: id ? { level: id.level, country: id.country, attestor: id.attestor, expires: id.expires } : null,
      domains: domains.get(a) || [],
      status: authorStatus(a),
      restricted: isRestricted(a),
      revoked: revoked.get(a) ?? null,
      fork_seq: store.forkSeq.get(a) ?? null,
      record_count: (store.logs.get(a) || []).length,
      hidden_listing_count: sellerListings.filter((l) => l.status === 'hidden').length,
    });
  }

  let redactedCount = 0;
  for (const e of logEntries) if (!e.r.body) redactedCount++;
  const stats = {
    records: logEntries.length,
    authors: authorIds.size,
    products: productViews.size,
    listings: listingViews.size,
    reviews: [...reviewsByProduct.values()].reduce((n, l) => n + l.length, 0),
    forks: store.forkSeq.size,
    invalid: store.rejectedCount,
    redacted: redactedCount,
  };

  return { productViews, listingViews, authorViews, stats, now };
}

const STOP = new Set(['the', 'a', 'an', 'and', 'for', 'of', 'with']);

export function searchState(state, q, { sort = 'relevance' } = {}) {
  const terms = String(q || '')
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter((t) => t && !STOP.has(t));
  const hits = [];
  for (const p of state.productViews.values()) {
    const name = p.name?.value ?? '';
    const brand = p.brand?.value ?? '';
    const category = p.category?.value ?? null;
    let score = 0;
    const why = [];
    if (terms.length) {
      const nameL = name.toLowerCase();
      const brandL = brand.toLowerCase();
      const catL = (category || '').toLowerCase();
      let matched = 0;
      for (const t of terms) {
        if (p.gtin && p.gtin.replace(/^0+/, '') === t.replace(/^0+/, '')) {
          score += 10;
          why.push(`GTIN matches "${t}"`);
          matched++;
        } else if (nameL.split(/[^\p{L}\p{N}]+/u).includes(t)) {
          score += 3;
          why.push(`name contains "${t}"`);
          matched++;
        } else if (nameL.includes(t)) {
          score += 2;
          why.push(`name contains "${t}"`);
          matched++;
        } else if (brandL.includes(t)) {
          score += 2;
          why.push(`brand matches "${t}"`);
          matched++;
        } else if (catL.includes(t)) {
          score += 1;
          why.push(`category matches "${t}"`);
          matched++;
        }
      }
      if (matched < terms.length) continue;
    } else {
      why.push('all products');
    }
    const vi = p.ratings.verified_identity;
    if (vi.count) {
      const bonus = Math.round(vi.mean * Math.log2(1 + vi.count) * 25) / 100;
      score += bonus;
      why.push(`${vi.count} identity-verified ${vi.count === 1 ? 'rating' : 'ratings'}, mean ${vi.mean}`);
    }
    let minPrice = null;
    let best = -1;
    for (const [c, r] of Object.entries(p.price_range)) {
      if (r.count > best && r.min != null) {
        best = r.count;
        minPrice = { amount: r.min, currency: c };
      }
    }
    hits.push({
      key: p.key,
      name,
      brand,
      category,
      score: Math.round(score * 100) / 100,
      why,
      rating_mean: vi.mean,
      rating_count: vi.count,
      verified_purchase_count: p.ratings.verified_purchase.count,
      verified_purchase_mean: p.ratings.verified_purchase.mean,
      min_price: minPrice,
    });
  }
  const tie = (a, b) => CMP(a.key, b.key);
  const sorters = {
    relevance: (a, b) => b.score - a.score || tie(a, b),
    price: (a, b) => (a.min_price ? 0 : 1) - (b.min_price ? 0 : 1) || CMP(a.min_price?.currency ?? '', b.min_price?.currency ?? '') || (a.min_price?.amount ?? 0) - (b.min_price?.amount ?? 0) || tie(a, b),
    rating_count: (a, b) => b.rating_count - a.rating_count || tie(a, b),
    rating_mean: (a, b) => (b.rating_mean ?? -1) - (a.rating_mean ?? -1) || b.rating_count - a.rating_count || tie(a, b),
  };
  hits.sort(sorters[sort] || sorters.relevance);
  return hits;
}

export function policyHashOf(policy, canonicalJson, normalizePolicy) {
  return sha256Hex(canonicalJson(normalizePolicy(policy)));
}

export { CONTROL_KINDS };
