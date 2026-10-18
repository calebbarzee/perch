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
// badge is a button that shows the sentence, so touch and keyboard users can
// read it. The sentence is also the badge's accessible description.
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
  restricted: (a, policy) => {
    const r = a?.restriction;
    if (!r || r.reason === 'forked_key') return 'Restricted: this person has a key with a conflicting history. Their listings are left out of price figures.';
    return `${fmt.plural(r.hidden_listings, "of this seller's listings is", "of this seller's listings are")} hidden, flagged by ${fmt.plural(r.distinct_flaggers, 'verified person', 'verified people')}. Your policy restricts a seller at ${fmt.plural(policy?.seller_hidden_listing_limit ?? 0, 'hidden listing')} flagged by ${fmt.plural(policy?.seller_min_distinct_flaggers ?? 0, 'person', 'people')}.`;
  },
  purchase: {
    verified: "The reviewer cites a receipt signed by the listing's seller, and both identities are verified under your policy.",
    none: 'The review cites no receipt. It counts in the rating but not as a verified purchase.',
    receipt_invalid: "The cited receipt is not signed by the seller of the named listing, so it is not accepted.",
    wrong_product: 'The cited receipt names a listing of another product.',
    seller_unverified: "The receipt is valid, but the seller's identity is not verified under your policy.",
    buyer_unverified: "The receipt is valid, but the reviewer's identity is not verified under your policy.",
    self: 'The seller of the listing wrote this review.',
  },
};

// "Restricted seller" for people with listings, "Restricted" otherwise.
export function restrictedLabel(view, { seller = false } = {}) {
  const isSeller = seller || view?.role === 'seller' || (view?.hidden_listing_count ?? 0) > 0 || (view?.restriction?.hidden_listings ?? 0) > 0;
  return isSeller ? 'Restricted seller' : 'Restricted';
}

// A badge: icon + short label. With a sentence, the badge is a disclosure
// button: pressing it shows the sentence below, and the sentence is the
// button's accessible description.
let badgeSeq = 0;
export function badge(tone, label, iconName, sentence = null) {
  if (!sentence) {
    return h('span', { class: `badge badge-${tone}` }, iconName ? icon(iconName) : null, label);
  }
  const id = `badge-s-${++badgeSeq}`;
  const text = h('span', { class: 'badge-sentence', id, hidden: true, text: sentence });
  const button = h('button', {
    type: 'button',
    class: `badge badge-${tone} badge-button`,
    'aria-expanded': 'false',
    'aria-describedby': id,
    on: {
      click: (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        text.hidden = !text.hidden;
        button.setAttribute('aria-expanded', String(!text.hidden));
      },
    },
  }, iconName ? icon(iconName) : null, label);
  return h('span', { class: 'badge-wrap' }, button, text);
}

export function identityBadge(verified) {
  if (!verified) return badge('muted', 'Identity not verified', 'shield', SENTENCE.notVerified);
  const label = verified.level === 'business' ? 'Business verified' : 'Identity verified';
  return badge('verified', label, 'shield', verified.attestor ? SENTENCE.identity(verified) : null);
}

export function statusBadge(status, view = null) {
  switch (status) {
    case 'forked':
      return badge('bad', 'Conflicting history', 'fork', SENTENCE.forked(view));
    case 'revoked':
      return badge('bad', 'Key revoked', 'key', SENTENCE.revoked(view));
    case 'retired':
      return badge('muted', 'Retired', 'clock', SENTENCE.retired(view));
    default:
      return null;
  }
}

export function restrictedBadge(view, policy, { seller = false } = {}) {
  return badge('bad', restrictedLabel(view, { seller }), 'ban', SENTENCE.restricted(view, policy));
}

// The active policy, for badge sentences in chips. Set by the app.
let activePolicy = null;
export function setActivePolicy(p) {
  activePolicy = p;
}

// Author chip (docs/ux.md 6.1): fingerprint, name as the author states it,
// verification, status, short key. The key identifies the author; the name
// is a claim. Data comes from the AuthorView when the lookup is set. The
// link covers fingerprint, name, and key; badges are buttons beside it.
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
  const who = [fingerprint(id, 16), nameEl, legal, hex(id, { n: 8, cls: 'author-key' })];
  const label = `${name || 'Unnamed author'}, key ${fmt.shortHex(id)}`;
  const main = link
    ? h('a', { href: routes.author(id), class: 'author-link', 'aria-label': label }, who)
    : h('span', { class: 'author-link' }, who);
  return h('span', { class: 'author-chip' },
    main,
    identityBadge(v && v.attestor ? v : v ? { level: v.level } : null),
    st && st !== 'ok' ? statusBadge(st, view) : null,
    view?.restricted ? restrictedBadge(view, activePolicy) : null,
  );
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

// Filled and outline stars differ in shape, not only color, and the number
// is shown as text (WCAG 1.4.1).
export function stars(rating) {
  const el = h('span', { class: 'stars' });
  const glyphs = h('span', { class: 'star-glyphs', 'aria-hidden': 'true' });
  for (let i = 1; i <= 5; i++) glyphs.append(h('span', { class: i <= rating ? 'star on' : 'star', text: i <= rating ? '★' : '☆' }));
  el.append(glyphs, h('span', { class: 'star-text', text: `${rating} of 5` }));
  return el;
}

const FLAG_EXCLUDED = {
  self: 'same person as the author of the flagged record',
  product_seller: 'by a seller of this product',
  unverified: 'identity not verified',
  duplicate_person: 'same person as an earlier flag',
  forked: "flagger's key has a conflicting history",
  retracted: 'withdrawn by the flagger',
  unavailable: 'flag text removed by a node',
  superseded: 'an older version of the same flag',
};

// Every flag on a record, with who flagged and whether it counts. Loaded
// when the user opens the list.
export function flagList(client, rootId) {
  const details = h('details', { class: 'flag-list' }, h('summary', { text: 'Show flags' }));
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
    const ap = appealBlock(view, owner);
    if (ap) parts.push(ap);
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

// The appeal of a flagged record, with its date and text (moderation.md 5.1).
export function appealBlock(view, owner) {
  if (view.appeal) {
    return h('div', { class: 'appeal', dataset: { testid: 'appeal' } },
      h('p', {}, `The ${owner} appealed on ${fmt.date(view.appeal.ts)}:`),
      userText('blockquote', view.appeal.note),
    );
  }
  if (view.appealed) return h('p', { class: 'appeal', dataset: { testid: 'appeal' }, text: `The ${owner} appealed.` });
  return null;
}

// Flags and appeal on a record that is not hidden (docs/ux.md 6.5, 8.3
// item 6): the counts, a "Show flags" list, and the appeal.
export function flagSummary(view, kind, client, rootId) {
  const f = view.flags || { count: 0, counted: 0, reasons: {} };
  if (!f.count && !view.appeal && !view.appealed) return null;
  const owner = kind === 'listing' ? 'seller' : 'author';
  const reasons = fmt.reasonsText(f.reasons);
  return h('div', { class: 'flag-summary', dataset: { testid: 'flag-summary' } },
    f.count
      ? h('p', {}, icon('flag'), ` ${fmt.plural(f.count, 'flag')}, ${fmt.number(f.counted)} counted under your policy${reasons ? `. Reasons: ${reasons}` : ''}.`)
      : null,
    f.count && client && rootId && typeof client.flags === 'function' ? flagList(client, rootId) : null,
    appealBlock(view, owner),
  );
}

// "How this was computed" (docs/ux.md 6.4, T2): the rule, what was left
// out, the policy fields used with their values, the policy hash, and the
// time used.
export function computedDisclosure({ rule, exclusions = [], policyFields = [], policy, policyHash }) {
  const now = policy?.now;
  return h('details', { class: 'computed', dataset: { testid: 'computed' } },
    h('summary', { text: 'How this was computed' }),
    rule ? h('p', { text: rule }) : null,
    exclusions.length ? h('p', { text: `Left out: ${exclusions.join('; ')}.` }) : null,
    policyFields.length
      ? h('dl', { class: 'facts compact' },
          policyFields.map(([k, v]) => h('div', { class: 'fact-row' }, h('dt', {}, h('code', { text: k })), h('dd', { text: typeof v === 'boolean' ? (v ? 'true' : 'false') : String(v ?? 'not set') }))),
        )
      : null,
    h('p', { class: 'hint' },
      'Policy ', h('a', { href: '#/policy' }, hex(policyHash || '', { n: 16 })),
      `. Time used: ${now != null ? `${fmt.dateTime(now)} (pinned)` : 'the current time on this device'}.`,
    ),
  );
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

// Rows of [label, value, absent text]. A null value shows the absent text,
// "Not stated" by default (T5: "None" would read as "ships nowhere").
export function definitionList(rows) {
  const dl = h('dl', { class: 'facts' });
  for (const [k, v, absent = 'Not stated'] of rows) {
    if (v === undefined) continue;
    dl.append(h('div', { class: 'fact-row' }, h('dt', { text: k }), h('dd', {}, v ?? h('span', { class: 'muted', text: absent }))));
  }
  return dl;
}

export const PURCHASE_TEXT = {
  verified: 'Verified purchase',
  none: 'No receipt',
  receipt_invalid: "Receipt not accepted: not signed by the listing's seller",
  wrong_product: 'Receipt is for a different product',
  seller_unverified: "Receipt from a seller whose identity is not verified",
  buyer_unverified: "Receipt valid, but the reviewer's identity is not verified",
  self: 'Seller reviewing their own listing',
};

export const EXCLUDED_TEXT = {
  hidden: 'hidden by moderation',
  unavailable: 'content removed by a node',
  retracted: 'withdrawn by its author',
  seller: 'written by a seller of this product',
  forked: "author's key has a conflicting history",
  revoked: 'written after the key was reported stolen',
  superseded: 'the same person has a newer review of this product',
};

// Why a listing is not in the price summary (ListingView.price_excluded_reason).
export const PRICE_EXCLUDED_TEXT = {
  domain_unverified: 'shop domain not verified',
  availability: 'not in stock',
  seller_restricted: 'seller is restricted',
  hidden: 'hidden by moderation',
  retracted: 'withdrawn by the seller',
  unavailable: 'content removed by a node',
};

// Why views ignore a record (RecordStatus.reason and ignored; docs/ux.md 12.3).
const IGNORED_TEXT = {
  'replaces:not_found': 'It replaces a record that is not on this device.',
  'replaces:author': 'It replaces a record by another author.',
  'replaces:seq': 'It replaces a record that is not older.',
  'replaces:kind': 'It replaces a record of another kind.',
  'replaces:not_replaceable': 'This kind of record cannot be replaced.',
  'retract:author': "It withdraws another author's record.",
  'retract:seq': 'It withdraws a record that is not older.',
  'retract:target_kind': 'This kind of record cannot be withdrawn.',
  'appeal:author': 'It appeals a record by another author.',
  'ref:listing_kind': 'It refers to a listing that is not a listing.',
  unavailable: 'Its content was removed by a node.',
  unknown_kind: 'This version of Perch does not know this kind of record.',
  future: 'Its stated time is in the future.',
  forked: "It is at or after the author's conflicting history.",
  revoked: 'It was signed after the key was reported stolen.',
  retired: 'It was signed after the author retired the key.',
  retracted: 'It is a new version of a withdrawn record.',
  not_after_seq: "Your policy ignores this attestor's records after a set number.",
};

export function ignoredText(code) {
  if (!code) return null;
  if (code.startsWith('schema:')) return `The ${code.slice(7)} field breaks the format rules.`;
  if (code.startsWith('fixed:')) return `It changes ${code.slice(6)}, which cannot change after publishing.`;
  return IGNORED_TEXT[code] || code;
}

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
