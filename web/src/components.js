// Shared UI pieces. All record text goes through userText()/bdi().

import { h, bdi, userText, externalLink } from './dom.js';
import { fingerprint } from './fingerprint.js';
import * as fmt from './format.js';
import { settings } from './settings.js';

// Icons: 16 x 16, stroke-based, decorative (aria-hidden). Meaning is always
// carried by the text next to them.
const ICONS = {
  check: 'M3.5 8.5l3 3 6-7',
  shield: 'M8 1.8l5 2v4c0 3.2-2.2 5.4-5 6.4-2.8-1-5-3.2-5-6.4v-4l5-2z M5.6 8.2l1.7 1.7 3.2-3.4',
  receipt: 'M4 1.8h8v12.4l-2-1.2-2 1.2-2-1.2-2 1.2z M6 5h4 M6 7.5h4 M6 10h2.5',
  eyeOff: 'M2 2l12 12 M6.4 6.5a2 2 0 002.9 2.8 M4.3 4.5C3 5.3 2 6.6 1.5 8c1.1 2.9 3.6 4.8 6.5 4.8 1.3 0 2.5-.4 3.5-1 M7 3.3c.3 0 .7-.1 1-.1 2.9 0 5.4 1.9 6.5 4.8-.3.9-.8 1.7-1.4 2.4',
  alert: 'M8 2l6.5 11.5h-13z M8 6.5v3.2 M8 11.6v.1',
  ban: 'M8 1.8a6.2 6.2 0 110 12.4 6.2 6.2 0 010-12.4z M3.6 3.6l8.8 8.8',
  undo: 'M4 6.5h6a3.5 3.5 0 010 7H6 M6.5 3.8L3.8 6.5l2.7 2.7',
  fork: 'M5 2.5v5.5c0 1.5 1.2 2.5 3 2.5s3-1 3-2.5V2.5 M8 10.5v3',
  key: 'M10.5 1.8a3.7 3.7 0 11-3.3 5.4L2 12.4V14h2.2v-1.5h1.5V11h1.4l1-1',
  globe: 'M8 1.8a6.2 6.2 0 110 12.4A6.2 6.2 0 018 1.8z M1.8 8h12.4 M8 1.8c1.8 1.9 2.6 4 2.6 6.2S9.8 12.3 8 14.2C6.2 12.3 5.4 10.2 5.4 8S6.2 3.7 8 1.8z',
  info: 'M8 1.8a6.2 6.2 0 110 12.4A6.2 6.2 0 018 1.8z M8 7.2v4 M8 4.8v.1',
  flag: 'M3.5 14.5V2 M3.5 2.5h8l-1.8 3 1.8 3h-8',
  clock: 'M8 1.8a6.2 6.2 0 110 12.4A6.2 6.2 0 018 1.8z M8 4.5V8l2.5 1.5',
  image: 'M2 3h12v10H2z M2 11l3.5-3.5 3 3 2-2L14 12 M10.5 6.2v.1',
  external: 'M9 2.5h4.5V7 M13.5 2.5L7.5 8.5 M11.5 9.5v4h-9v-9h4',
};

export function icon(name, cls = '') {
  const svg = h('svg:svg', {
    class: `icon ${cls}`.trim(),
    width: 16,
    height: 16,
    viewBox: '0 0 16 16',
    'aria-hidden': 'true',
    focusable: 'false',
  });
  svg.append(h('svg:path', { d: ICONS[name] || ICONS.info }));
  return svg;
}

// Hash route links. Keys and IDs are URI-encoded.
export const routes = {
  search: (q, sort) => {
    const p = new URLSearchParams();
    if (q) p.set('q', q);
    if (sort && sort !== 'relevance') p.set('sort', sort);
    const s = p.toString();
    return '#/search' + (s ? '?' + s : '');
  },
  product: (key) => `#/product/${encodeURIComponent(key)}`,
  listing: (id) => `#/listing/${id}`,
  author: (id) => `#/author/${id}`,
  record: (id) => `#/data/${id}`,
  publish: (kind, params = {}) => {
    const p = new URLSearchParams({ kind, ...params });
    return `#/publish?${p}`;
  },
};

export function hex(value, { n = 8, cls = '' } = {}) {
  return h('code', { class: `hex ${cls}`.trim(), title: value, text: fmt.shortHex(value, n) });
}

export function fullHex(value) {
  return h('code', { class: 'hex hex-full', text: value });
}

export function recordLink(id, label) {
  return h('a', { href: routes.record(id), class: 'record-link' }, label ? label : hex(id));
}

// Looks up an AuthorView by key. Set by the app; cached until the data or
// policy changes (clearAuthorCache).
let authorLookup = null;
const authorCache = new Map();
export function setAuthorLookup(fn) {
  authorLookup = fn;
  authorCache.clear();
}
export function clearAuthorCache() {
  authorCache.clear();
}
export function lookupAuthor(id) {
  if (!authorLookup || !id) return null;
  if (!authorCache.has(id)) {
    let v = null;
    try {
      v = authorLookup(id);
    } catch {
      v = null;
    }
    authorCache.set(id, v);
  }
  return authorCache.get(id);
}

/** Display name of an attestor or other author, or its short key. */
export function displayName(id) {
  return lookupAuthor(id)?.name ?? `key ${fmt.shortHex(id)}…`;
}

// One sentence per badge (docs/ux.md 6.2). Badges show a short label; the
// sentence is the badge's tooltip and accessible text, and appears in full
// on the Author screen.
export const SENTENCE = {
  identity: (v) =>
    v.level === 'business'
      ? `Business ${v.legal_name ?? 'name not given'}${v.country ? ` (${v.country})` : ''} verified by ${displayName(v.attestor)} until ${fmt.date(v.expires)}, from a company registry.`
      : `Identity verified by ${displayName(v.attestor)} until ${fmt.date(v.expires)}. Level: person.`,
  notVerified: 'No attestor in your policy has verified this key.',
  domain: (d) => `${displayName(d.attestor)} confirmed until ${fmt.date(d.expires)} that this seller controls ${d.domain}.`,
  domainNot: (host) => `No attestor in your policy has confirmed that this seller controls ${host || 'this address'}. Check the address before you pay.`,
  forked: (a) => `This key signed two different records at position ${a?.fork_seq ?? '?'}. Its records from that position on are ignored.`,
  revoked: (a) => a?.revoked
    ? `${displayName(a.revoked.attestor)} reported this key as compromised. Records after #${a.revoked.valid_through_seq} are ignored.`
    : 'An attestor reported this key as compromised.',
  retired: (a) => a?.retired
    ? `The owner ended this key at record #${a.retired.seq}. Later records are ignored.${a.retired.successor ? ` Successor: ${fmt.shortHex(a.retired.successor)}… Verification does not transfer.` : ''}`
    : 'The owner ended this key. Later records are ignored.',
  restricted: (a, policy) => a?.restriction
    ? `${a.restriction.hidden_listings} of this seller's listings are hidden, flagged by ${a.restriction.distinct_flaggers} verified people. Policy limits: ${policy?.seller_hidden_listing_limit ?? '?'} listings and ${policy?.seller_min_distinct_flaggers ?? '?'} people.`
    : 'This person has a key with a conflicting history.',
};

// A badge: icon + short label. With a sentence, the sentence is the tooltip
// and, unless `quiet`, also read by screen readers after the label.
export function badge(tone, label, iconName, sentence = null, { quiet = false } = {}) {
  return h('span', { class: `badge badge-${tone}`, title: sentence || null },
    iconName ? icon(iconName) : null,
    label,
    sentence && !quiet ? h('span', { class: 'visually-hidden', text: `: ${sentence}` }) : null,
  );
}

export function identityBadge(verified, opts = {}) {
  if (!verified) return badge('muted', 'Identity not verified', 'shield', SENTENCE.notVerified, opts);
  const label = verified.level === 'business' ? 'Business verified' : 'Identity verified';
  return badge('verified', label, 'shield', SENTENCE.identity(verified), opts);
}

export function statusBadge(status, view = null, opts = {}) {
  switch (status) {
    case 'forked':
      return badge('bad', 'Conflicting history', 'fork', SENTENCE.forked(view), opts);
    case 'revoked':
      return badge('bad', 'Key revoked', 'key', SENTENCE.revoked(view), opts);
    case 'retired':
      return badge('muted', 'Retired', 'clock', SENTENCE.retired(view), opts);
    default:
      return null;
  }
}

// Author chip (docs/ux.md 6.1): fingerprint, name as the author states it,
// verification, status, short key. The key identifies the author; the name
// is a claim. Data comes from the AuthorView when the lookup is set.
export function authorChip(id, name, { verified = false, status = 'ok', link = true } = {}) {
  const view = lookupAuthor(id);
  if (name == null) name = view?.name ?? null;
  const v = view ? view.verified : verified ? {} : null;
  const st = view ? view.status : status;
  const nameEl = name
    ? bdi(name, { class: 'author-name' })
    : h('span', { class: 'author-name unnamed', text: view?.profile?.status === 'hidden' ? 'Name hidden by moderation' : 'Unnamed author' });
  const legal = v?.level === 'business' && v.legal_name && v.legal_name !== name
    ? h('span', { class: 'legal-name' }, ' (', bdi(v.legal_name), v.country ? `, ${v.country}` : '', ')')
    : null;
  const parts = [
    fingerprint(id, 16),
    nameEl,
    legal,
    v && v.attestor ? identityBadge(v, { quiet: true }) : v ? badge('verified', 'Identity verified', 'shield') : identityBadge(null, { quiet: true }),
    st && st !== 'ok' ? statusBadge(st, view, { quiet: true }) : null,
    view?.restricted ? badge('bad', 'Restricted seller', 'ban', null) : null,
    hex(id, { n: 8, cls: 'author-key' }),
  ];
  const label = `${name || 'Unnamed author'}, ${v ? (v.level === 'business' ? 'business verified' : 'identity verified') : 'identity not verified'}${st && st !== 'ok' ? `, ${st === 'forked' ? 'conflicting history' : st}` : ''}, key ${fmt.shortHex(id)}`;
  return link
    ? h('a', { href: routes.author(id), class: 'author-chip', 'aria-label': label }, parts)
    : h('span', { class: 'author-chip' }, parts);
}

// Notice block: tone = info | good | warn | bad | muted.
export function notice(tone, title, ...body) {
  const iconName = { info: 'info', good: 'check', warn: 'alert', bad: 'ban', muted: 'eyeOff', hidden: 'eyeOff' }[tone] || 'info';
  return h(
    'div',
    { class: `notice notice-${tone}`, role: tone === 'bad' ? 'alert' : null },
    icon(iconName, 'notice-icon'),
    h('div', { class: 'notice-body' }, title ? h('p', { class: 'notice-title', text: title }) : null, ...body),
  );
}

export function stars(rating) {
  const el = h('span', { class: 'stars', role: 'img', 'aria-label': `${rating} of 5 stars` });
  for (let i = 1; i <= 5; i++) el.append(h('span', { class: i <= rating ? 'star on' : 'star', 'aria-hidden': 'true', text: '★' }));
  return el;
}

const FLAG_EXCLUDED = {
  self: 'same person as the author of the flagged record',
  product_seller: 'by a seller of this product',
  unverified: 'identity not verified',
  duplicate_person: 'same person as an earlier flag',
  forked: "flagger's log is forked",
  retracted: 'withdrawn by the flagger',
  unavailable: 'flag text removed by a node',
  superseded: 'an older version of the same flag',
};

// Every flag on a record, with who flagged and whether it counts. Loaded
// when the user opens the list.
export function flagList(client, rootId) {
  const details = h('details', { class: 'flag-list' }, h('summary', { text: 'Show who flagged' }));
  details.addEventListener('toggle', () => {
    if (!details.open || details.dataset.loaded) return;
    details.dataset.loaded = '1';
    let flags = [];
    try {
      flags = client.flags(rootId) || [];
    } catch (e) {
      details.append(h('p', { class: 'hint', text: `Flags could not be listed (${e.message}).` }));
      return;
    }
    details.append(
      h('ul', { class: 'plain flags' },
        flags.map((f) =>
          h('li', { class: f.counted ? 'counted' : 'not-counted' },
            authorChip(f.author, f.author_name, { verified: f.author_verified }),
            ' ',
            f.reason ? h('strong', { text: fmt.FLAG_REASONS[f.reason] || f.reason }) : null,
            h('span', { class: 'muted', text: ` ${fmt.date(f.ts)}` }),
            ' ',
            recordLink(f.record, 'record'),
            f.counted ? null : h('span', { class: 'hint', text: ` Not counted: ${FLAG_EXCLUDED[f.excluded_reason] || f.excluded_reason}.` }),
            f.note ? userText('p', f.note, { class: 'flag-note' }) : null,
          ),
        ),
      ),
    );
  }, { once: false });
  return details;
}

// Moderation and availability notice for a listing, review, product, or
// profile (7.6, 5.3). `view` has status, flags, appeal, and redaction.
export function moderationNotice(view, kind, policy, client = null, rootId = null) {
  const what = { listing: 'listing', review: 'review', product: 'product', profile: 'profile' }[kind] || 'record';
  const owner = kind === 'listing' ? 'seller' : 'author';
  if (view.status === 'hidden') {
    const threshold = policy?.flag_thresholds?.[kind] ?? policy?.flag_threshold ?? null;
    const counted = view.flags.counted;
    const verifiedOnly = policy?.flags_require_verified;
    const parts = [
      h('p', {}, `${fmt.plural(counted, verifiedOnly ? 'identity-verified person' : 'person', verifiedOnly ? 'identity-verified people' : 'people')} flagged this ${what}. `
        + (threshold != null ? `Your policy hides a ${what} at ${threshold}. ` : '')
        + `${fmt.plural(view.flags.count, 'flag')} in total.`),
    ];
    const reasons = fmt.reasonsText(view.flags.reasons);
    if (reasons) parts.push(h('p', {}, `Reasons: ${reasons}.`));
    if (client && rootId && typeof client.flags === 'function') parts.push(flagList(client, rootId));
    if (view.appeal) {
      parts.push(
        h('div', { class: 'appeal' },
          h('p', {}, `The ${owner} appealed on ${fmt.date(view.appeal.ts)}:`),
          userText('blockquote', view.appeal.note),
        ),
      );
    } else if (view.appealed) {
      parts.push(h('p', { text: `The ${owner} appealed.` }));
    }
    parts.push(h('p', { class: 'hint', text: kind === 'review'
      ? 'Hidden reviews stay in the data and still count in the rating. Only the text is collapsed. Moderation follows your policy on this device.'
      : kind === 'listing'
        ? 'Hidden listings stay in the data. Their prices are left out of price ranges. Moderation follows your policy on this device.'
        : 'Hidden records stay in the data. Moderation follows your policy on this device.' }));
    const n = notice('hidden', `Hidden by moderation`, ...parts);
    n.dataset.status = 'hidden';
    return n;
  }
  if (view.status === 'unavailable') {
    const r = view.redaction;
    const parts = [];
    if (r && r.record) {
      parts.push(h('p', {}, 'A node operator removed the text of this ', what, ' and published a notice. Reason: ', fmt.FLAG_REASONS[r.reason] || r.reason || 'not given', '.'));
      if (r.note) parts.push(userText('blockquote', r.note));
      parts.push(h('p', {}, 'Notice by ', authorChip(r.by, null), ' in record ', recordLink(r.record), '.'));
    } else {
      parts.push(h('p', { text: `The connected nodes do not have the text of this ${what}. No removal notice was published.` }));
    }
    parts.push(h('p', { class: 'hint', text: 'The signed header remains, so the log is still complete.' }));
    const n = notice('muted', 'Text removed', ...parts);
    n.dataset.status = 'unavailable';
    return n;
  }
  if (view.status === 'retracted') {
    const n = notice('muted', `Withdrawn by its ${owner}`, h('p', { text: `The ${what} remains in the data but no longer counts.` }));
    n.dataset.status = 'retracted';
    return n;
  }
  return null;
}

// A fact with its sources and conflicting values (Fact<T> in 7.8).
export function factValue(fact, { tag = 'span', render } = {}) {
  if (!fact) return h('span', { class: 'muted', text: 'Not stated' });
  const value = render ? render(fact.value) : userText(tag, fact.value);
  const nSources = fact.sources.length;
  const nConflicts = fact.conflicts.length;
  const summary = nConflicts
    ? `${fmt.plural(nSources, 'source')}, ${fmt.plural(nConflicts, 'other value')}`
    : fmt.plural(nSources, 'source');
  const details = h(
    'details',
    { class: ['fact-sources', nConflicts ? 'has-conflicts' : null] },
    h('summary', {}, summary),
    sourceList(fact.sources),
    nConflicts
      ? h('div', { class: 'conflicts' },
          h('p', { class: 'conflicts-title', text: 'Other values' }),
          ...fact.conflicts.map((c) => h('div', { class: 'conflict' }, userText('p', c.value, { class: 'conflict-value' }), sourceList(c.sources))),
        )
      : null,
  );
  return h('span', { class: 'fact' }, value, ' ', details);
}

export function sourceList(sources) {
  return h(
    'ul',
    { class: 'source-list' },
    sources.map((s) => h('li', {}, authorChip(s.author, s.name ?? null, { verified: s.verified }), ' ', recordLink(s.record, 'record'))),
  );
}

async function sha256OfBytes(bytes) {
  if (globalThis.crypto?.subtle) {
    const d = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
    return Array.from(d, (b) => b.toString(16).padStart(2, '0')).join('');
  }
  const { sha256Hex } = await import('./sha256.js');
  return sha256Hex(bytes);
}

// Image from a record (docs/ux.md 8.2 item 4). Images from unverified
// authors load only on request, because loading reveals the viewer's IP
// address to the image host (ARCHITECTURE 10). An image with a stated
// sha256 is fetched, hashed, and shown from a blob: URL only if it matches.
// When the host blocks the fetch (CORS), the image is shown unchecked and
// labeled "Hash not checked".
export function productImage(img, alt) {
  const auto = img.author_verified && settings().autoLoadVerifiedImages;
  const wrap = h('figure', { class: 'product-image', dataset: { hash: img.sha256 ? 'pending' : 'none' } });
  const host = fmt.host(img.url);
  const hostEl = () => h('span', { class: 'host', text: host || 'unknown host' });
  const caption = (...extra) => h('figcaption', {}, hostEl(), img.author_verified ? '' : ' (from an unverified author)', ...extra);
  const showPlain = (label, state) => {
    const el = h('img', { src: img.url, alt: alt || '', referrerpolicy: 'no-referrer', loading: 'lazy', decoding: 'async' });
    el.addEventListener('error', () => wrap.replaceChildren(h('p', { class: 'image-error' }, 'The image from ', hostEl(), ' did not load.')));
    wrap.dataset.hash = state;
    wrap.replaceChildren(el, caption(label ? h('span', { class: 'hash-note', text: ` ${label}` }) : null));
  };
  const load = async () => {
    if (!img.sha256) {
      showPlain(null, 'none');
      return;
    }
    wrap.replaceChildren(h('p', { class: 'muted', text: 'Loading and checking the image.' }));
    let bytes;
    try {
      const res = await fetch(img.url, { credentials: 'omit', referrerPolicy: 'no-referrer', mode: 'cors' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      bytes = new Uint8Array(await res.arrayBuffer());
    } catch {
      showPlain('Hash not checked: the image host does not allow the check.', 'unchecked');
      return;
    }
    const digest = await sha256OfBytes(bytes);
    if (digest !== img.sha256) {
      wrap.dataset.hash = 'mismatch';
      wrap.replaceChildren(notice('bad', 'Image not shown', h('p', {}, 'The image at ', hostEl(), ' does not match its stated hash.')));
      return;
    }
    const url = URL.createObjectURL(new Blob([bytes]));
    const el = h('img', { src: url, alt: alt || '', decoding: 'async' });
    wrap.dataset.hash = 'match';
    wrap.replaceChildren(el, caption(h('span', { class: 'hash-note', text: ' Hash checked.' })));
  };
  if (auto) {
    load();
  } else {
    wrap.append(
      icon('image', 'placeholder-icon'),
      h('p', {}, 'Image from ', hostEl(), img.author_verified ? ', stated by a verified author.' : ', stated by an unverified author.'),
      h('p', { class: 'hint' }, 'Loading it shows your IP address to that host.'),
      h('button', { type: 'button', class: 'button small', on: { click: load } }, 'Load image'),
    );
  }
  return wrap;
}

export function priceText(price) {
  return price ? fmt.money(price.amount, price.currency) : '–';
}

export function emptyState(title, body, action) {
  return h('div', { class: 'empty' }, h('p', { class: 'empty-title', text: title }), body ? h('p', { text: body }) : null, action || null);
}

export function urlLink(url, label) {
  return externalLink(url, label);
}

export function definitionList(rows) {
  const dl = h('dl', { class: 'facts' });
  for (const [k, v] of rows) {
    if (v === undefined) continue;
    dl.append(h('div', { class: 'fact-row' }, h('dt', { text: k }), h('dd', {}, v ?? h('span', { class: 'muted', text: 'None' }))));
  }
  return dl;
}

export const PURCHASE_TEXT = {
  verified: 'Verified purchase',
  none: null,
  receipt_invalid: "Receipt does not verify under the listing's seller",
  wrong_product: 'Receipt is for a different product',
  seller_unverified: "Receipt from a seller whose identity is not verified",
  buyer_unverified: "Receipt valid, but the reviewer's identity is not verified",
  self: 'Seller reviewing their own listing',
};

export const EXCLUDED_TEXT = {
  hidden: 'hidden by moderation',
  unavailable: 'text removed',
  retracted: 'withdrawn by its author',
  seller: 'written by a seller of this product',
  forked: "author's log is forked",
  revoked: 'written after the key was reported stolen',
  superseded: 'an older review by the same author',
};

// Receipt share of a seller (docs/ux.md 8.3 item 7, threat-model R8).
export function receiptShare(authorView) {
  const sr = authorView?.seller_reviews;
  if (!sr || !sr.total) return null;
  return h('div', { class: 'receipt-share', dataset: { testid: 'receipt-share' } },
    h('p', { text: `${fmt.number(sr.with_valid_receipt)} of ${fmt.plural(sr.total, 'counted review')} of this seller's products cite a valid receipt from this seller.` }),
    h('details', { class: 'computed' },
      h('summary', { text: 'What this means' }),
      h('p', { text: 'Sellers choose whom to give receipts. A low share can mean receipts are given selectively, for example only to happy buyers.' }),
    ),
  );
}
