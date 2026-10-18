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

// Author name with fingerprint and key prefix. The name is the author's own
// claim; the key prefix is what identifies them.
export function authorChip(id, name, { verified = false, status = 'ok', link = true } = {}) {
  const parts = [
    fingerprint(id, 16),
    name ? bdi(name, { class: 'author-name' }) : h('span', { class: 'author-name unnamed', text: 'No profile' }),
    hex(id, { n: 8, cls: 'author-key' }),
  ];
  if (verified) parts.push(badge('verified', 'Identity verified', 'shield'));
  if (status && status !== 'ok') parts.push(statusBadge(status));
  const el = link ? h('a', { href: routes.author(id), class: 'author-chip' }, parts) : h('span', { class: 'author-chip' }, parts);
  return el;
}

export function badge(tone, text, iconName) {
  return h('span', { class: `badge badge-${tone}` }, iconName ? icon(iconName) : null, text);
}

export function statusBadge(status) {
  switch (status) {
    case 'forked':
      return badge('bad', 'Forked log', 'fork');
    case 'revoked':
      return badge('bad', 'Key revoked', 'key');
    case 'retired':
      return badge('muted', 'Retired', 'clock');
    default:
      return null;
  }
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

// Moderation and availability notice for a listing or review (7.6, 5.3).
export function moderationNotice(view, kind, policy) {
  const what = kind === 'listing' ? 'listing' : 'review';
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
    if (view.appeal) {
      parts.push(
        h('div', { class: 'appeal' },
          h('p', {}, `The ${kind === 'listing' ? 'seller' : 'author'} appealed on ${fmt.date(view.appeal.ts)}:`),
          userText('blockquote', view.appeal.note),
        ),
      );
    } else if (view.appealed) {
      parts.push(h('p', { text: `The ${kind === 'listing' ? 'seller' : 'author'} appealed.` }));
    }
    parts.push(h('p', { class: 'hint', text: 'Hidden records stay in the data. Moderation follows your policy on this device.' }));
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
    const n = notice('muted', `Withdrawn by its ${kind === 'listing' ? 'seller' : 'author'}`, h('p', { text: `The ${what} remains in the data but no longer counts.` }));
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

// Image from a record. Unverified authors' images load only on request,
// because loading reveals the viewer's IP address to the image host (10).
export function productImage(img, alt) {
  const auto = img.author_verified && settings().autoLoadVerifiedImages;
  const wrap = h('figure', { class: 'product-image' });
  const host = fmt.host(img.url);
  const load = () => {
    const el = h('img', { src: img.url, alt: alt || '', referrerpolicy: 'no-referrer', loading: 'lazy', decoding: 'async' });
    el.addEventListener('error', () => {
      wrap.replaceChildren(h('p', { class: 'image-error', text: `The image from ${host} did not load.` }));
    });
    wrap.replaceChildren(el, h('figcaption', {}, host ? h('span', { class: 'host', text: host }) : null, img.author_verified ? '' : ' (unverified source)'));
  };
  if (auto) {
    load();
  } else {
    wrap.append(
      icon('image', 'placeholder-icon'),
      h('p', {}, img.author_verified ? 'Image from a verified author.' : 'Image from an unverified author.'),
      h('p', { class: 'hint' }, 'Loading it shows your IP address to ', h('span', { class: 'host', text: host || 'the image host' }), '.'),
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
