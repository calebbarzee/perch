// Perch panel: an extension page shown in an iframe on product pages.
//
// 1. The GTIN comes from the content script (URL hash).
// 2. The product key is gtin:<14 digits>. Only the first 4 hex characters of
//    sha256(key) are sent to each configured node (/v1/lookup/{prefix}), so a
//    node learns a bucket of about 1/65,536 of all products, not the product.
// 3. Each node returns JSONL records. The wasm client verifies every
//    signature and hash in a sparse store and computes the views here, with
//    the policy bundled in this extension. A node cannot change the numbers;
//    it can only withhold records. Querying several nodes exposes that.

import {
  perchClientClass,
  bundledPolicy,
  loadSettings,
  appProductUrl,
  nodeLabel,
  sha256Hex,
} from './src/client.js';

const root = document.getElementById('panel');
const params = new URLSearchParams(location.hash.slice(1));
const gtin = params.get('gtin') || '';

function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === 'text') el.textContent = String(v);
    else if (k === 'on') for (const [ev, fn] of Object.entries(v)) el.addEventListener(ev, fn);
    else el.setAttribute(k, v === true ? '' : String(v));
  }
  for (const c of children.flat(Infinity)) {
    if (c == null || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

function reportHeight() {
  const height = Math.ceil(document.documentElement.getBoundingClientRect().height);
  parent.postMessage({ type: 'perch:height', height }, '*');
}
new ResizeObserver(reportHeight).observe(document.documentElement);

// Minor units come from the client's ISO 4217 table, not Intl defaults.
const minorUnits = new Map();
const moneyFmt = new Map();
function money(amount, currency) {
  const digits = minorUnits.get(currency);
  if (digits == null) return `${amount} ${currency} (minor units)`;
  if (!moneyFmt.has(currency)) {
    try {
      moneyFmt.set(currency, new Intl.NumberFormat(undefined, { style: 'currency', currency, minimumFractionDigits: digits, maximumFractionDigits: digits }));
    } catch {
      moneyFmt.set(currency, null);
    }
  }
  const f = moneyFmt.get(currency);
  if (!f) return `${amount} ${currency} (minor units)`;
  return f.format(amount / 10 ** digits);
}

const mean = (m) => (m == null ? '–' : String(Math.round(m * 100) / 100));
const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

function header(extra) {
  return h('header', { class: 'head' },
    h('p', { class: 'brand' }, h('span', { class: 'dot', 'aria-hidden': 'true' }), 'Perch'),
    extra,
    h('button', { type: 'button', class: 'close', 'aria-label': 'Close the Perch panel', on: { click: () => parent.postMessage({ type: 'perch:close' }, '*') } }, '×'),
  );
}

function show(...children) {
  root.replaceChildren(...children.flat(Infinity).filter((c) => c != null && c !== false));
  requestAnimationFrame(reportHeight);
}

async function lookup(node, prefix, signal) {
  const url = new URL(`/v1/lookup/${prefix}`, node);
  const res = await fetch(url, { signal, credentials: 'omit', referrerPolicy: 'no-referrer', cache: 'no-store' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.text();
}

// Numbers shown in the panel, from a ProductView.
function summarize(view) {
  if (!view) return null;
  const r = view.ratings;
  const listings = view.listings.filter((l) => l.status === 'visible').length;
  return {
    name: view.name?.value ?? view.key,
    identity: r.verified_identity,
    purchase: r.verified_purchase,
    all: { count: r.count, mean: r.mean },
    listings,
    hiddenListings: view.listings.length - listings,
    priceRange: view.price_range,
  };
}

const sameNumbers = (a, b) =>
  JSON.stringify(a && [a.identity.count, a.identity.mean, a.purchase.count, a.all.count, a.listings, a.priceRange]) ===
  JSON.stringify(b && [b.identity.count, b.identity.mean, b.purchase.count, b.all.count, b.listings, b.priceRange]);

async function main() {
  if (!/^\d{8,14}$/.test(gtin)) {
    show(header(), h('p', { class: 'msg', text: 'No product number on this page.' }));
    return;
  }
  const key = `gtin:${gtin.padStart(14, '0')}`;
  show(header(), h('p', { class: 'msg', role: 'status', text: 'Checking Perch records for this product.' }));

  const [settings, policy, PerchClient] = await Promise.all([loadSettings(), bundledPolicy(), perchClientClass()]);
  const prefix = (await sha256Hex(key)).slice(0, 4);

  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 10000);
  const perNode = await Promise.all(
    settings.nodes.map(async (node) => {
      try {
        return { node, jsonl: await lookup(node, prefix, ctrl.signal) };
      } catch (e) {
        return { node, error: e.name === 'AbortError' ? 'no answer within 10 s' : e.message };
      }
    }),
  );
  clearTimeout(timer);

  // One sparse client per node, plus one with the union of all records.
  const union = await PerchClient.create({ sparse: true });
  union.setPolicy(policy);
  for (const c of union.currencies()) minorUnits.set(c.code, c.minor_units);
  let rejected = 0;
  for (const n of perNode) {
    if (n.error) continue;
    const c = await PerchClient.create({ sparse: true });
    c.setPolicy(policy);
    const res = c.ingest(n.jsonl);
    rejected += res.rejected;
    n.rejected = res.rejected;
    n.summary = summarize(c.product(key));
    union.ingest(n.jsonl);
  }
  const ok = perNode.filter((n) => !n.error);
  const total = summarize(union.product(key));
  const policyHash = union.policyHash();

  const sources = h('p', { class: 'sources' },
    'Records from ',
    ok.length ? ok.map((n, i) => [i ? ', ' : '', h('span', { class: 'host', text: nodeLabel(n.node) })]) : 'no node',
    '. Signatures checked and numbers computed in this browser with the bundled policy ',
    h('code', { title: policyHash, text: policyHash.slice(0, 8) }),
    '.',
  );
  const failed = perNode.filter((n) => n.error);
  const failures = failed.length
    ? h('p', { class: 'warn' }, failed.map((n, i) => [i ? ' ' : '', h('span', { class: 'host', text: nodeLabel(n.node) }), ` did not answer (${n.error}).`]))
    : null;

  if (!ok.length) {
    show(header(), h('p', { class: 'msg', text: 'No Perch node answered.' }), failures, optionsLink());
    return;
  }
  if (!total) {
    show(header(), h('p', { class: 'msg', text: `No Perch records for GTIN ${gtin}.` }), sources, failures);
    return;
  }

  const link = appProductUrl(settings, key);
  const ratingLine = total.identity.count
    ? h('p', { class: 'rating', 'data-testid': 'rating' },
        h('strong', { class: 'big', text: mean(total.identity.mean) }),
        h('span', { class: 'of', text: ' / 5' }),
        h('span', { class: 'sub', text: ` from ${plural(total.identity.count, 'identity-verified reviewer')}` }),
      )
    : h('p', { class: 'rating none', 'data-testid': 'rating', text: 'No ratings from identity-verified reviewers.' });

  const split = h('dl', { class: 'split' },
    h('div', { 'data-testid': 'purchase' }, h('dt', { text: 'Verified purchases' }), h('dd', { text: total.purchase.count ? `${mean(total.purchase.mean)} from ${total.purchase.count}` : 'none' })),
    h('div', { 'data-testid': 'all' }, h('dt', { text: 'All counted reviews' }), h('dd', { text: total.all.count ? `${mean(total.all.mean)} from ${total.all.count}` : 'none' })),
  );

  const ranges = Object.entries(total.priceRange).filter(([, r]) => r.count > 0);
  const price = ranges.length
    ? ranges.map(([cur, r]) =>
        h('p', { class: 'price', 'data-testid': 'price', 'data-currency': cur },
          h('strong', { text: r.min === r.max ? money(r.min, cur) : `${money(r.min, cur)} to ${money(r.max, cur)}` }),
          h('span', { class: 'sub', text: ` across ${plural(r.count, 'listing')} on verified seller domains` }),
        ))
    : h('p', { class: 'price none', 'data-testid': 'price', text: 'No price from a verified seller domain.' });

  const listingsLine = h('p', { class: 'listings', 'data-testid': 'listings' },
    `${plural(total.listings, 'listing')}`,
    total.hiddenListings ? `, plus ${total.hiddenListings} hidden by moderation` : '',
    '.',
  );

  const disagree = ok.filter((n) => !sameNumbers(n.summary, total));
  const disagreement = ok.length > 1 && disagree.length
    ? h('div', { class: 'disagree', role: 'note', 'data-testid': 'disagree' },
        h('p', { class: 'warn-title', text: 'Nodes returned different records' }),
        h('ul', {}, ok.map((n) =>
          h('li', {}, h('span', { class: 'host', text: nodeLabel(n.node) }), ': ',
            n.summary ? `${n.summary.identity.count} verified ratings, ${n.summary.listings} listings` : 'no records for this product'),
        )),
        h('p', { class: 'sub', text: 'The numbers above combine all records. A node with fewer records may be behind or hiding some.' }),
      )
    : null;

  show(
    header(h('p', { class: 'name', title: total.name }, h('bdi', { text: total.name }))),
    ratingLine,
    split,
    price,
    listingsLine,
    disagreement,
    rejected ? h('p', { class: 'warn', text: `${plural(rejected, 'record')} failed verification and were ignored.` }) : null,
    failures,
    sources,
    link ? h('p', { class: 'open' }, h('a', { href: link, target: '_blank', rel: 'noopener noreferrer', 'data-testid': 'open', text: 'Open in Perch' })) : null,
  );
}

function optionsLink() {
  return h('p', {}, h('a', { href: chrome.runtime.getURL('options.html'), target: '_blank', rel: 'noopener', text: 'Choose nodes' }));
}

main().catch((e) => {
  show(header(), h('p', { class: 'warn', text: `Perch could not check this product: ${e.message || e}` }));
});
