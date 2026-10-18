// Perch panel (docs/ux.md 9.2-9.4). Three modes:
// - tab:   in an iframe on the shop's page, collapsed to a small tab until
//          the user opens it (click, or Alt+Shift+P);
// - popup: the toolbar button's popup; asks the page for its GTINs;
// - page:  opened directly (tests, or "Open" links), GTIN in the URL hash.
//
// Lookup: only the first 4 hex characters of sha256(product key) go to each
// configured node (/v1/lookup/{prefix}). Each node answers with JSONL. The
// wasm client verifies every record in a sparse store and computes the views
// here with the bundled policy. A node cannot change the numbers; it can only
// leave records out, which comparing nodes exposes.

import { perchClientClass, bundledPolicy, loadSettings, appProductUrl, appUrl, nodeLabel, sha256Hex } from './src/client.js';

const root = document.getElementById('panel');
const params = new URLSearchParams(location.hash.slice(1));
const mode = params.get('mode') || 'page';
let gtin = params.get('gtin') || '';
let host = params.get('host') || '';
let expanded = mode !== 'tab' || params.get('expanded') === '1';
document.documentElement.dataset.mode = mode;

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

const toParent = (msg) => {
  if (mode === 'tab') parent.postMessage(msg, '*');
};

function reportHeight() {
  if (mode !== 'tab' || !expanded) return;
  toParent({ type: 'perch:height', height: Math.ceil(document.documentElement.getBoundingClientRect().height) });
}
new ResizeObserver(reportHeight).observe(document.documentElement);

// Money from the client's ISO 4217 table, not Intl defaults (docs/ux.md 6.9).
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
  return f ? f.format(amount / 10 ** digits) : `${amount} ${currency} (minor units)`;
}
const dateFmt = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' });
const mean = (m) => (m == null ? '–' : new Intl.NumberFormat(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 2 }).format(m));
const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
const AVAILABILITY = { in_stock: 'in stock', out_of_stock: 'out of stock', preorder: 'preorder', discontinued: 'discontinued' };

// GTIN as printed under the barcode (docs/ux.md 8.2).
function printed(g14) {
  if (g14.startsWith('000000')) return g14.slice(6);
  if (g14.startsWith('00')) return g14.slice(2);
  if (g14.startsWith('0')) return g14.slice(1);
  return g14;
}

let state = { tab: 'Perch …', content: [h('p', { class: 'msg', role: 'status', text: 'Checking Perch records.' })] };

function header(name) {
  return h('header', { class: 'head' },
    h('p', { class: 'brand' }, h('span', { class: 'dot', 'aria-hidden': 'true' }), 'Perch'),
    name ? h('p', { class: 'name', title: name }, h('bdi', { text: name })) : null,
    mode === 'tab'
      ? h('button', { type: 'button', class: 'close', 'aria-label': 'Collapse the Perch panel', on: { click: () => setExpanded(false) } }, '×')
      : null,
  );
}

function render() {
  if (mode === 'tab' && !expanded) {
    root.replaceChildren(
      h('button', { type: 'button', class: 'tab', 'data-testid': 'tab', 'aria-expanded': 'false', on: { click: () => setExpanded(true) } },
        h('span', { class: 'dot', 'aria-hidden': 'true' }), state.tab),
    );
    return;
  }
  root.replaceChildren(header(state.name), ...state.content);
  requestAnimationFrame(reportHeight);
}

function setExpanded(value, { notify = true } = {}) {
  expanded = value;
  if (notify) toParent({ type: value ? 'perch:expand' : 'perch:collapse' });
  render();
  if (value) root.querySelector('.close')?.focus();
  else root.querySelector('.tab')?.focus();
}

window.addEventListener('message', (ev) => {
  if (ev.source !== parent || ev.data?.type !== 'perch:expanded') return;
  if (Boolean(ev.data.value) !== expanded) setExpanded(Boolean(ev.data.value), { notify: false });
});
document.addEventListener('keydown', (ev) => {
  if (ev.key === 'Escape' && mode === 'tab' && expanded) setExpanded(false);
});

function show(tab, content, name = null) {
  state = { tab, content: content.flat(Infinity).filter((c) => c != null && c !== false), name };
  render();
}

async function lookup(node, prefix, signal) {
  const res = await fetch(new URL(`/v1/lookup/${prefix}`, node), { signal, credentials: 'omit', referrerPolicy: 'no-referrer', cache: 'no-store' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return res.text();
}

function summarize(view) {
  if (!view) return null;
  return {
    identity: view.ratings.verified_identity,
    listings: view.listings.filter((l) => l.status === 'visible').length,
    priceRange: view.price_range,
  };
}

const sameNumbers = (a, b) =>
  JSON.stringify(a && [a.identity.count, a.identity.mean, a.listings, a.priceRange]) ===
  JSON.stringify(b && [b.identity.count, b.identity.mean, b.listings, b.priceRange]);

const hostMatches = (h1, domain) => h1 === domain || h1.endsWith(`.${domain}`);
const urlHost = (u) => {
  try {
    return new URL(u).hostname;
  } catch {
    return null;
  }
};

// Popup: ask the active tab for its GTINs.
async function fromActiveTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id) return null;
  return chrome.tabs.sendMessage(tab.id, { type: 'perch:get' }).catch(() => null);
}

async function main() {
  const settings = await loadSettings();
  if (!settings.setupDone) {
    show('Perch: set up', [
      h('p', { class: 'msg', text: 'Finish setting up Perch: choose the nodes to ask.' }),
      h('p', {}, h('a', { href: chrome.runtime.getURL('options.html'), target: '_blank', rel: 'noopener', text: 'Open settings' })),
    ]);
    return;
  }
  if (mode === 'popup' && !gtin) {
    const info = await fromActiveTab();
    host = info?.host || '';
    const list = info?.gtins || [];
    if (!list.length) {
      show('Perch', [h('p', { class: 'msg', text: 'No product number found on this page.' })]);
      return;
    }
    if (list.length > 1) {
      show('Perch', [
        h('p', { class: 'msg', text: `This page has ${list.length} product numbers. Choose one.` }),
        h('ul', { class: 'pick' }, list.map((g) => h('li', {}, h('button', { type: 'button', class: 'link', on: { click: () => { gtin = g; check(settings); } } }, `GTIN ${printed(g)}`)))),
      ]);
      return;
    }
    gtin = list[0];
  }
  await check(settings);
}

async function check(settings) {
  gtin = gtin.padStart(14, '0');
  if (!/^\d{14}$/.test(gtin)) {
    show('Perch', [h('p', { class: 'msg', text: 'No product number on this page.' })]);
    return;
  }
  const key = `gtin:${gtin}`;
  show('Perch …', [h('p', { class: 'msg', role: 'status', text: `Checking Perch records for GTIN ${printed(gtin)}.` })]);

  const [policy, PerchClient] = await Promise.all([bundledPolicy(), perchClientClass()]);
  const prefix = (await sha256Hex(key)).slice(0, 4);

  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 10000);
  const perNode = await Promise.all(
    settings.nodes.map(async (node) => {
      try {
        return { node, jsonl: await lookup(node, prefix, ctrl.signal) };
      } catch (e) {
        return { node, error: e.name === 'AbortError' ? 'timed out' : e.message };
      }
    }),
  );
  clearTimeout(timer);

  const union = await PerchClient.create({ sparse: true });
  union.setPolicy(policy);
  for (const c of union.currencies()) if (Number.isInteger(c.minor_units)) minorUnits.set(c.code, c.minor_units);
  let rejected = 0;
  for (const n of perNode) {
    if (n.error) continue;
    const c = await PerchClient.create({ sparse: true });
    c.setPolicy(policy);
    rejected += c.ingest(n.jsonl).rejected;
    n.summary = summarize(c.product(key));
    union.ingest(n.jsonl);
  }
  const ok = perNode.filter((n) => !n.error);
  const failed = perNode.filter((n) => n.error);
  const view = union.product(key);
  const policyHash = union.policyHash();

  const failures = failed.length
    ? h('p', { class: 'warn' }, failed.map((n, i) => [i ? ' ' : '', h('span', { class: 'host', text: nodeLabel(n.node) }), ` did not answer (${n.error}).`]))
    : null;
  const privacy = h('p', { class: 'privacy', 'data-testid': 'privacy' },
    'Sent: the hash prefix ', h('code', { text: prefix }), ` to ${plural(settings.nodes.length, 'node')}. Not sent: this page's address, the product, or cookies.`,
  );
  const app = appProductUrl(settings, key);

  if (!ok.length) {
    show('Perch: offline', [
      h('p', { class: 'msg', text: 'No Perch node answered.' }),
      failures,
      privacy,
      h('p', {}, h('a', { href: chrome.runtime.getURL('options.html'), target: '_blank', rel: 'noopener', text: 'Choose nodes' })),
    ]);
    return;
  }
  const sources = h('p', { class: 'sources' },
    `Checked ${plural(ok.length, 'node')}: `,
    ok.map((n, i) => [i ? ', ' : '', h('span', { class: 'host', text: nodeLabel(n.node) })]),
    '. Signatures checked and numbers computed in this browser with the bundled policy ',
    h('code', { title: policyHash, text: policyHash.slice(0, 8) }),
    '.',
  );
  if (!view) {
    show('Perch: no data', [
      h('p', { class: 'msg', text: `No Perch records for GTIN ${printed(gtin)} on the ${plural(ok.length, 'node')} checked.` }),
      appUrl(settings) ? h('p', {}, h('a', { href: `${appUrl(settings)}#/publish?kind=product&gtin=${printed(gtin)}`, target: '_blank', rel: 'noopener noreferrer', text: 'Add this product' })) : null,
      failures,
      sources,
      privacy,
    ], `GTIN ${printed(gtin)}`);
    return;
  }

  const r = view.ratings;
  const vi = r.verified_identity;
  const vp = r.verified_purchase;
  const ratingLine = vi.count
    ? h('p', { class: 'rating', 'data-testid': 'rating' },
        h('strong', { class: 'big', text: mean(vi.mean) }),
        h('span', { class: 'of', text: ' / 5' }),
        h('span', { class: 'sub', text: ` from ${plural(vi.count, 'identity-verified reviewer')}` }),
      )
    : h('p', { class: 'rating none', 'data-testid': 'rating', text: 'No rating from identity-verified reviewers.' });
  const split = h('dl', { class: 'split' },
    h('div', { 'data-testid': 'purchase' }, h('dt', { text: 'Verified purchases' }), h('dd', { text: vp.count ? `${mean(vp.mean)} from ${vp.count}` : 'none' })),
    h('div', { 'data-testid': 'all' }, h('dt', { text: 'All counted reviews' }), h('dd', { text: r.count ? `${mean(r.mean)} from ${r.count}` : 'none' })),
  );

  const ranges = Object.entries(view.price_range).sort((a, b) => b[1].count - a[1].count || (a[0] < b[0] ? -1 : 1));
  const prices = ranges.length
    ? ranges.map(([cur, pr]) =>
        h('div', { class: 'price', 'data-testid': 'price', 'data-currency': cur },
          pr.count
            ? h('p', {}, h('strong', { text: pr.min === pr.max ? money(pr.min, cur) : `${money(pr.min, cur)} to ${money(pr.max, cur)}` }), h('span', { class: 'sub', text: ` from ${plural(pr.count, 'listing')} on verified shop domains` }))
            : h('p', { class: 'sub', text: `No ${cur} price from a verified shop domain.` }),
          pr.excluded_unverified_domain ? h('p', { class: 'sub', text: `${plural(pr.excluded_unverified_domain, 'listing')} not counted: shop domain not verified.` }) : null,
        ))
    : h('p', { class: 'price none', 'data-testid': 'price', text: 'No current price from a verified shop domain.' });
  const visible = view.listings.filter((l) => l.status === 'visible').length;
  const listingsLine = h('p', { class: 'listings', 'data-testid': 'listings', text: `${plural(visible, 'listing')}.` });

  // This site (docs/ux.md 9.3 item 4). The host never leaves the browser.
  let site = null;
  if (host) {
    const verifiedHere = view.listings.find((l) => l.domain_verified && l.domain && hostMatches(host, l.domain.domain));
    const onHere = view.listings.find((l) => l.status === 'visible' && urlHost(l.url) && hostMatches(urlHost(l.url), host));
    const attestorName = (id) => union.authorView(id)?.name ?? `key ${id.slice(0, 8)}…`;
    site = h('div', { class: ['site', verifiedHere ? 'site-ok' : 'site-warn'].join(' '), 'data-testid': 'site' },
      verifiedHere
        ? h('p', {}, h('span', { class: 'host', text: host }), ' is verified as the shop of ', h('bdi', { text: verifiedHere.seller_name || `key ${verifiedHere.seller.slice(0, 8)}…` }), ` by ${attestorName(verifiedHere.domain.attestor)} until ${dateFmt.format(new Date(verifiedHere.domain.expires))}.`)
        : h('p', {}, 'No seller in Perch has a verified domain for ', h('span', { class: 'host', text: host }), '. Check the address before you pay.'),
      onHere && onHere.price ? h('p', { class: 'sub', text: `This site's listing: ${money(onHere.price.amount, onHere.price.currency)}, ${AVAILABILITY[onHere.availability] || onHere.availability}.` }) : null,
    );
  }

  // Moderation (item 5).
  const hiddenReviews = view.reviews.filter((x) => x.status === 'hidden').length;
  const hiddenListings = view.listings.filter((x) => x.status === 'hidden').length;
  const modParts = [hiddenReviews ? plural(hiddenReviews, 'review') : null, hiddenListings ? plural(hiddenListings, 'listing') : null].filter(Boolean);
  const moderation = modParts.length ? h('p', { class: 'sub', 'data-testid': 'moderation', text: `${modParts.join(' and ')} ${hiddenReviews + hiddenListings === 1 ? 'is' : 'are'} hidden by moderation.` }) : null;

  // Disagreement between nodes (item 6).
  const total = summarize(view);
  const disagree = ok.length > 1 && ok.some((n) => !sameNumbers(n.summary, total));
  const disagreement = disagree
    ? h('div', { class: 'disagree', role: 'note', 'data-testid': 'disagree' },
        h('p', { class: 'warn-title', text: 'Nodes returned different records' }),
        h('ul', {}, ok.map((n) =>
          h('li', {}, h('span', { class: 'host', text: nodeLabel(n.node) }), ': ',
            n.summary ? `${n.summary.identity.count} identity-verified ratings, ${n.summary.listings} listings` : 'no records for this product'),
        )),
        h('p', { class: 'sub', text: 'The numbers above combine the records from all nodes. A node with fewer records may be behind or leaving some out.' }),
      )
    : null;

  const name = view.name?.value || `GTIN ${printed(gtin)}`;
  show(
    vi.count ? `Perch ${mean(vi.mean)} (${vi.count})` : 'Perch: no rating',
    [
      h('p', { class: 'gtin', text: `GTIN ${printed(gtin)}` }),
      ratingLine,
      split,
      prices,
      listingsLine,
      site,
      moderation,
      disagreement,
      rejected ? h('p', { class: 'warn', text: `${plural(rejected, 'record')} failed verification and were ignored.` }) : null,
      failures,
      sources,
      privacy,
      app
        ? h('p', { class: 'open' },
            h('a', { href: app, target: '_blank', rel: 'noopener noreferrer', 'data-testid': 'open', text: 'Open in Perch' }),
            ' ',
            h('a', { href: `${appUrl(settings)}#/publish?kind=review&product=${encodeURIComponent(key)}`, target: '_blank', rel: 'noopener noreferrer', text: 'Write a review' }),
          )
        : null,
      mode === 'tab'
        ? h('p', {}, h('button', { type: 'button', class: 'link small', on: { click: () => toParent({ type: 'perch:hide-site' }) } }, 'Hide on this site'))
        : null,
    ],
    name,
  );
}

render();
main().catch((e) => {
  show('Perch: error', [h('p', { class: 'warn', text: `Perch could not check this product: ${e.message || e}` })]);
});
