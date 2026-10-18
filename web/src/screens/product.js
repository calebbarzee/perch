import { h, bdi, userText } from '../dom.js';
import * as fmt from '../format.js';
import {
  routes, authorChip, badge, notice, stars, moderationNotice, factValue, productImage,
  priceText, emptyState, definitionList, PURCHASE_TEXT, EXCLUDED_TEXT, PRICE_EXCLUDED_TEXT, icon, hex, recordLink, urlLink,
  SENTENCE, lookupAuthor, restrictedBadge, restrictedLabel, flagSummary, computedDisclosure,
} from '../components.js';
import { announce } from '../dom.js';
import { priceChart } from '../chart.js';

export default {
  id: 'product',
  nav: 'search',
  live: (ctx) => JSON.stringify(ctx.client.product(ctx.args.key)),
  render(ctx) {
    const { client, args } = ctx;
    const p = client.product(args.key);
    if (!p) {
      return h('section', { class: 'screen' },
        h('h1', { text: 'Product not found' }),
        emptyState(`No product with key ${args.key} is in your local data.`, 'Sync with more nodes on the Network screen, or search again.', h('a', { class: 'button', href: '#/search', text: 'Search' })),
      );
    }
    const policy = client.policy();
    const policyHash = client.policyHash();
    const section = h('article', { class: 'screen screen-product', dataset: { key: p.key } });

    // Header.
    section.append(
      h('header', { class: 'product-head' },
        h('p', { class: 'crumbs' }, h('a', { href: routes.search(p.brand?.value || ''), text: 'Search' }), p.category ? [' / ', h('span', { text: fmt.categoryText(p.category.value) })] : null),
        h('h1', {}, bdi(p.name?.value ?? p.key)),
        h('p', { class: 'product-sub' }, h('span', { class: 'brand' }, bdi(p.brand?.value ?? '')), ' ', h('span', { class: 'key', text: fmt.keyLabel(p.key) })),
      ),
    );

    // Product records hidden by moderation: the facts stay visible, because
    // other sellers' listings and reviews share the product key.
    if (p.moderation && p.moderation.status !== 'visible') {
      const firstSource = p.name?.sources?.[0]?.record;
      const rootId = firstSource ? client.recordStatus?.(firstSource)?.root ?? firstSource : null;
      const mod = moderationNotice(p.moderation, 'product', policy, client, rootId);
      if (mod) section.append(mod);
    }

    // Review filters, shared by the histogram buttons and the review list.
    const filter = reviewFilter(ctx, p);

    // Summary: rating and price side by side.
    section.append(h('div', { class: 'summary-grid' }, ratingPanel(p.ratings, filter, policy, policyHash), pricePanel(p, policy, policyHash)));

    // Listings.
    section.append(listingsSection(p, policy, client));

    // Price history.
    const currencies = [...new Set(p.listings.flatMap((l) => l.price_history.map((x) => x.currency)))].sort();
    if (currencies.length) {
      const charts = h('section', { class: 'block', 'aria-labelledby': 'h-history' }, h('h2', { id: 'h-history', text: 'Price history' }));
      const nowTs = policy.now ?? Date.now();
      for (const c of currencies) charts.append(priceChart({ listings: p.listings.filter((l) => l.status !== 'unavailable'), currency: c, now: nowTs, title: `Prices in ${c}` }));
      section.append(charts);
    }

    // Reviews.
    section.append(reviewsSection(p, policy, client, filter));

    // Details: facts with sources.
    section.append(detailsSection(p));

    return section;
  },
};

// Rating histogram (docs/ux.md 8.2 item 9, T6). Each row is a button that
// sets the star filter of the review list. With 1 to 5 reviews, counts are
// shown as text without bars; with 0, no histogram.
function histogram(hist, label, filter, source = 'identity') {
  const total = hist.reduce((a, b) => a + b, 0);
  if (!total) return null;
  const bars = total > 5;
  const max = Math.max(1, ...hist);
  return h('table', { class: ['histogram', bars ? null : 'no-bars'] },
    h('caption', { class: 'visually-hidden', text: label }),
    h('tbody', {},
      [5, 4, 3, 2, 1].map((star) => {
        const n = hist[star - 1];
        return h('tr', {},
          h('th', { scope: 'row' },
            h('button', {
              type: 'button',
              class: 'link-button',
              'aria-label': `${star} star${star === 1 ? '' : 's'}: ${n}. Show these reviews.`,
              // The histogram counts identity-verified people, so the
              // list shows the same set (m3).
              on: { click: () => filter?.set(source, star, { focus: true }) },
            }, `${star} star${star === 1 ? '' : 's'}`),
          ),
          bars ? h('td', { class: 'bar-cell', 'aria-hidden': 'true' }, h('span', { class: 'bar', dataset: { w: String(Math.round((n / max) * 100)) } })) : null,
          h('td', { class: 'num', text: fmt.number(n) }),
        );
      }),
    ),
  );
}

function sizeBars(root) {
  if (!root) return;
  for (const b of root.querySelectorAll('.bar[data-w]')) b.style.width = `${b.dataset.w}%`;
}

function ratingPanel(r, filter, policy, policyHash) {
  const vi = r.verified_identity;
  const vp = r.verified_purchase;
  const panel = h('section', { class: 'panel rating-panel', 'aria-labelledby': 'h-rating' },
    h('h2', { id: 'h-rating', text: 'Rating' }),
  );
  if (vi.count) {
    panel.append(
      h('p', { class: 'headline-rating', dataset: { testid: 'rating-verified' } },
        h('span', { class: 'big', text: fmt.mean(vi.mean) }),
        h('span', { class: 'of', text: ' / 5' }),
      ),
      h('p', { class: 'headline-sub', text: `from ${fmt.plural(vi.count, 'identity-verified reviewer')}` }),
    );
  } else {
    panel.append(h('p', { class: 'headline-rating none', text: 'No rating from identity-verified reviewers.' }));
  }
  const hist = histogram(vi.histogram, 'Ratings from identity-verified reviewers', filter);
  sizeBars(hist);
  if (hist) panel.append(hist);
  panel.append(
    h('dl', { class: 'secondary-ratings' },
      h('div', { dataset: { testid: 'rating-purchase' } },
        h('dt', {}, icon('receipt'), ' Verified purchases'),
        h('dd', { text: vp.count ? `${fmt.mean(vp.mean)} from ${fmt.plural(vp.count, 'review')}` : 'No verified-purchase reviews' }),
      ),
      h('div', { dataset: { testid: 'rating-all' } },
        h('dt', { text: 'All counted reviews' }),
        h('dd', { text: r.count ? `${fmt.mean(r.mean)} from ${fmt.plural(r.count, 'review')}` : 'No rating' }),
      ),
    ),
    h('p', { class: 'hint', text: 'All counted reviews include reviewers whose identity is not verified.' }),
  );
  const ex = Object.entries(r.excluded || {}).filter(([, n]) => n > 0);
  const exText = ex.map(([k, n]) => `${n} ${EXCLUDED_TEXT[k] || k}`);
  if (ex.length) {
    panel.append(h('p', { class: 'hint excluded-summary' }, `Not counted: ${exText.join('; ')}.`));
  }
  panel.append(
    h('p', { class: 'hint', text: 'Hidden reviews still count in ratings. Hiding collapses their text only.' }),
    computedDisclosure({
      rule: 'The headline is the mean of the latest counted review of each identity-verified person, so new keys cannot change it. A review counts unless it is withdrawn, removed by a node, written by a seller of this product, replaced by a newer review from the same person, or signed by a key with a conflicting history or after a revocation. Verified purchases also need a receipt signed by the listing\'s seller.',
      exclusions: exText,
      policyFields: [
        ['trusted_attestors', `${(policy.trusted_attestors || []).length} attestors`],
        ['verified_purchase_requires_verified_buyer', policy.verified_purchase_requires_verified_buyer],
        ['flag_thresholds.review', policy.flag_thresholds?.review ?? policy.flag_threshold],
      ],
      policy,
      policyHash,
    }),
  );
  return panel;
}

function pricePanel(p, policy, policyHash) {
  const panel = h('section', { class: 'panel price-panel', 'aria-labelledby': 'h-price' }, h('h2', { id: 'h-price', text: 'Price' }));
  const entries = Object.entries(p.price_range).sort((a, b) => b[1].count - a[1].count || (a[0] < b[0] ? -1 : 1));
  if (!entries.length) {
    panel.append(h('p', { class: 'muted', text: 'No listing in stock or on preorder.' }));
  }
  for (const [cur, r] of entries) {
    const row = h('div', { class: 'price-range', dataset: { currency: cur } });
    if (r.count) {
      row.append(
        h('p', { class: 'range' },
          h('strong', { class: 'big', text: fmt.money(r.min, cur) }),
          r.max !== r.min ? [h('span', { class: 'to', text: ' to ' }), h('strong', { text: fmt.money(r.max, cur) })] : null,
        ),
        h('p', { class: 'headline-sub', text: `Median ${fmt.money(r.median, cur)} across ${fmt.plural(r.count, 'listing')} in stock or on preorder, on verified seller domains` }),
      );
    } else {
      row.append(h('p', { class: 'muted', text: `No ${cur} listings on verified seller domains.` }));
    }
    if (r.excluded_unverified_domain) {
      row.append(h('p', { class: 'hint', text: `${fmt.plural(r.excluded_unverified_domain, 'listing')} not in the range: the seller's domain is not verified.` }));
    }
    panel.append(row);
  }
  const left = p.listings.filter((l) => !l.feeds_prices && l.price_excluded_reason !== 'domain_unverified');
  if (left.length) {
    const reasons = [...new Set(left.map((l) => PRICE_EXCLUDED_TEXT[l.price_excluded_reason] || l.price_excluded_reason))];
    panel.append(h('p', { class: 'hint', text: `${fmt.plural(left.length, 'listing')} not counted: ${reasons.join(', ')}.` }));
  }
  const counts = {};
  for (const l of p.listings) if (!l.feeds_prices) counts[l.price_excluded_reason] = (counts[l.price_excluded_reason] || 0) + 1;
  panel.append(computedDisclosure({
    rule: 'The range runs from the lowest to the highest current price of listings in the price summary: visible, in stock or on preorder, from a seller who is not restricted, on a shop domain an attestor in your policy verified. The median is the middle of those prices.',
    exclusions: Object.entries(counts).map(([k, n]) => `${n} ${PRICE_EXCLUDED_TEXT[k] || k}`),
    policyFields: [
      ['price_requires_domain_verified', policy.price_requires_domain_verified],
      ['flag_threshold', policy.flag_threshold],
      ['seller_hidden_listing_limit', policy.seller_hidden_listing_limit],
      ['seller_min_distinct_flaggers', policy.seller_min_distinct_flaggers],
    ],
    policy,
    policyHash,
  }));
  return panel;
}

// Listings in two parts (B1): those in the price summary first, then the
// rest under one heading per reason. Perch does not sell, so the heading is
// "Listings", not "Where to buy".
const GROUP_ORDER = ['domain_unverified', 'availability', 'seller_restricted', 'hidden', 'unavailable', 'retracted'];

function listingsSection(p, policy, client) {
  const n = p.listings.length;
  const sec = h('section', { class: 'block', 'aria-labelledby': 'h-listings' }, h('h2', { id: 'h-listings', text: `Listings (${n})` }));
  if (!n) {
    sec.append(h('p', { class: 'muted', text: 'No seller has listed this product.' }));
    return sec;
  }
  const feeding = p.listings.filter((l) => l.feeds_prices);
  const others = p.listings.filter((l) => !l.feeds_prices);
  sec.append(h('p', { class: 'muted listings-count', text: `${fmt.plural(n, 'listing')}: ${feeding.length} in the price summary, ${others.length} not.` }));

  sec.append(h('h3', { id: 'h-listings-feeding', text: `In the price summary (${feeding.length})` }));
  if (feeding.length) {
    sec.append(h('ul', { class: 'listings', dataset: { group: 'feeds' } }, feeding.map((l) => listingCard(l, policy, { client }))));
  } else {
    sec.append(h('p', { class: 'muted', text: 'No listing is in the price summary.' }));
  }

  const wide = typeof matchMedia === 'function' ? matchMedia('(min-width: 600px)').matches : true;
  const reasons = [...new Set(others.map((l) => l.price_excluded_reason))].sort((a, b) => GROUP_ORDER.indexOf(a) - GROUP_ORDER.indexOf(b));
  for (const reason of reasons) {
    const group = others.filter((l) => l.price_excluded_reason === reason);
    sec.append(
      h('details', { class: 'listing-group', open: wide, dataset: { group: reason } },
        h('summary', {}, h('h3', { class: 'inline', text: `Not in the price summary: ${PRICE_EXCLUDED_TEXT[reason] || reason} (${group.length})` })),
        h('ul', { class: 'listings' }, group.map((l) => listingCard(l, policy, { client }))),
      ),
    );
  }
  return sec;
}

// One listing (docs/ux.md 8.2 item 7). Hidden listings put the moderation
// notice first and keep title, price, and shop link behind "Show listing"
// (M1). Withdrawn listings are one line (m2).
export function listingCard(l, policy, { showProduct = false, client = null } = {}) {
  const li = h('li', { class: ['listing', `status-${l.status}`], dataset: { listing: l.id, status: l.status, feeds: String(Boolean(l.feeds_prices)) } });
  const seller = authorChip(l.seller, l.seller_name, { verified: l.seller_verified, status: l.seller_status });
  const details = h('a', { href: routes.listing(l.id), text: 'Listing details' });
  if (l.status === 'retracted') {
    li.append(h('p', { class: 'listing-line' }, seller, ' ', h('span', { text: `Withdrawn by the seller on ${fmt.date(l.updated_ts)}.` }), ' ', details, ' ', recordLink(l.latest, 'Record')));
    return li;
  }
  const badges = h('p', { class: 'badges' },
    l.domain_verified && l.domain
      ? badge('good', `Shop domain verified: ${l.domain.domain}`, 'globe', SENTENCE.domain(l.domain))
      : badge('warn', 'Domain not verified', 'globe', SENTENCE.domainNot(fmt.host(l.url))),
    l.seller_restricted ? restrictedBadge(lookupAuthor(l.seller), policy, { seller: true }) : null,
  );
  const head = h('div', { class: 'listing-head' },
    h('div', { class: 'listing-who' }, l.status === 'hidden' ? null : seller, l.title ? userText('p', l.title, { class: 'listing-title' }) : null),
    h('div', { class: 'listing-price' },
      h('p', { class: 'price', text: priceText(l.price) }),
      h('p', { class: 'muted' }, `${fmt.AVAILABILITY[l.availability] || l.availability || ''}, ${fmt.CONDITION[l.condition] || l.condition || ''}`),
      shipsText(l) ? h('p', { class: 'muted ships', text: shipsText(l) }) : null,
    ),
  );
  const links = h('p', { class: 'listing-links' },
    l.url && l.status !== 'unavailable' ? urlLink(l.url, 'Seller page') : null,
    ' ',
    details,
    showProduct ? [' ', h('a', { href: routes.product(l.product), text: 'Product' })] : null,
    h('span', { class: 'muted', text: ` Updated ${fmt.date(l.updated_ts)}` }),
  );
  const mod = moderationNotice(l, 'listing', policy, client, l.id);

  if (l.status === 'hidden') {
    const body = h('div', { class: 'listing-hidden-body', hidden: true }, head, badges, links);
    const toggle = h('button', {
      type: 'button',
      class: 'button small',
      'aria-expanded': 'false',
      on: {
        click: () => {
          body.hidden = !body.hidden;
          toggle.setAttribute('aria-expanded', String(!body.hidden));
          toggle.textContent = body.hidden ? 'Show listing' : 'Hide listing';
        },
      },
    }, 'Show listing');
    li.append(mod, h('p', { class: 'listing-who' }, seller, ' ', h('span', { class: 'muted', text: 'Listing hidden by moderation.' })), toggle, body);
    return li;
  }

  li.append(head, badges);
  if (l.seller_restricted) {
    const view = lookupAuthor(l.seller);
    li.append(notice('bad', restrictedLabel(view, { seller: true }), h('p', {}, SENTENCE.restricted(view, policy), ' ', h('a', { href: routes.author(l.seller), text: 'See the seller' }), '.')));
  }
  if (mod) li.append(mod);
  if (!l.feeds_prices && l.price_excluded_reason && l.status === 'visible') {
    li.append(h('p', { class: 'hint price-excluded', text: `Not in the price summary: ${PRICE_EXCLUDED_TEXT[l.price_excluded_reason] || l.price_excluded_reason}.` }));
  }
  const fs = flagSummary(l, 'listing', client, l.id);
  if (fs) li.append(fs);
  li.append(links);
  return li;
}

const SOURCES = [
  ['everyone', 'Everyone', () => true],
  ['purchase', 'Verified purchases', (r) => r.verified_purchase],
  ['identity', 'Identity-verified people', (r) => r.author_verified],
];

// Review filter state, kept in the URL (?reviews=&stars=) with replaceState,
// so Back and shared links restore it without re-rendering the screen.
function reviewFilter(ctx, p) {
  const f = {
    source: SOURCES.some(([v]) => v === ctx.params.reviews) ? ctx.params.reviews : 'everyone',
    stars: /^[1-5]$/.test(ctx.params.stars || '') ? Number(ctx.params.stars) : null,
    listeners: [],
    reviews: p.reviews,
    onChange(fn) {
      this.listeners.push(fn);
    },
    set(source, stars, opts = {}) {
      this.source = source;
      this.stars = stars;
      const params = new URLSearchParams();
      if (source !== 'everyone') params.set('reviews', source);
      if (stars) params.set('stars', String(stars));
      const base = location.hash.split('?')[0];
      history.replaceState(null, '', base + (params.toString() ? `?${params}` : ''));
      for (const fn of this.listeners) fn(opts);
    },
    setStars(stars, opts) {
      this.set(this.source, stars, opts);
    },
  };
  return f;
}

function reviewsSection(p, policy, client, filter) {
  const hidden = p.reviews.filter((r) => r.status === 'hidden').length;
  const sec = h('section', { class: 'block', 'aria-labelledby': 'h-reviews' },
    h('div', { class: 'block-head' },
      h('h2', { id: 'h-reviews', tabindex: '-1', text: `Reviews (${p.reviews.length})` }),
      h('a', { class: 'button', href: routes.publish('review', { product: p.key }), text: 'Write a review' }),
    ),
  );
  if (!p.reviews.length) {
    sec.append(h('p', { class: 'muted', text: 'No reviews yet.' }));
    return sec;
  }
  sec.append(h('p', { class: 'muted', text: `${fmt.plural(p.reviews.length, 'review')}${hidden ? `, ${hidden} hidden` : ''}, newest first.` }));

  const sourceGroup = h('fieldset', { class: 'radio-row' }, h('legend', { text: 'Show reviews from' }));
  for (const [v, label, pred] of SOURCES) {
    const id = `rf-src-${v}`;
    const input = h('input', { type: 'radio', name: 'rf-source', id, value: v, checked: filter.source === v });
    input.addEventListener('change', () => filter.set(v, filter.stars));
    sourceGroup.append(h('span', { class: 'radio' }, input, h('label', { for: id, text: `${label} (${p.reviews.filter(pred).length})` })));
  }
  const starGroup = h('fieldset', { class: 'radio-row' }, h('legend', { text: 'Stars' }));
  const starInputs = [];
  const renderStars = () => {
    const pred = SOURCES.find(([v]) => v === filter.source)[2];
    const base = p.reviews.filter(pred);
    starGroup.replaceChildren(h('legend', { text: 'Stars' }));
    starInputs.length = 0;
    for (const v of [null, 5, 4, 3, 2, 1]) {
      const id = `rf-star-${v ?? 'all'}`;
      const n = v ? base.filter((r) => r.rating === v).length : base.length;
      const input = h('input', { type: 'radio', name: 'rf-stars', id, value: String(v ?? ''), checked: filter.stars === v });
      input.addEventListener('change', () => filter.set(filter.source, v));
      starInputs.push(input);
      starGroup.append(h('span', { class: 'radio' }, input, h('label', { for: id, text: v ? `${v} (${n})` : `All (${n})` })));
    }
  };
  const list = h('ul', { class: 'reviews' });
  const shown = h('p', { class: 'result-count', role: 'status' });
  for (const r of p.reviews) list.append(reviewItem(r, policy, client));
  const apply = (opts = {}) => {
    const pred = SOURCES.find(([v]) => v === filter.source)[2];
    let n = 0;
    for (const li of list.children) {
      const r = p.reviews.find((x) => x.id === li.dataset.review);
      const show = pred(r) && (!filter.stars || r.rating === filter.stars);
      li.hidden = !show;
      if (show) n++;
    }
    for (const input of sourceGroup.querySelectorAll('input')) input.checked = input.value === filter.source;
    renderStars();
    shown.textContent = n === p.reviews.length ? '' : `Showing ${fmt.plural(n, 'review')}.`;
    if (opts.focus) {
      document.getElementById('h-reviews')?.focus();
      announce(`Showing ${fmt.plural(n, 'review')} with ${filter.stars} star${filter.stars === 1 ? '' : 's'}.`);
    }
  };
  filter.onChange(apply);
  sec.append(h('div', { class: 'review-filter' }, sourceGroup, starGroup), shown, list);
  apply();
  return sec;
}

export function shipsText(l) {
  const parts = [];
  if (l.location) parts.push(`Ships from ${l.location.country}${l.location.region ? `, ${l.location.region}` : ''}`);
  if (l.ships_to?.length) {
    const list = l.ships_to.length > 8 ? `${l.ships_to.slice(0, 8).join(', ')} and ${l.ships_to.length - 8} more` : l.ships_to.join(', ');
    parts.push(parts.length ? `to ${list}` : `Ships to ${list}`);
  }
  return parts.join(' ');
}

export function reviewItem(r, policy, client = null) {
  const li = h('li', {
    class: ['review', `status-${r.status}`, r.counted ? null : 'not-counted'],
    dataset: { review: r.id, status: r.status, counted: String(r.counted), purchase: r.purchase, identity: String(r.author_verified) },
  });
  const purchaseText = PURCHASE_TEXT[r.purchase];
  li.append(
    h('div', { class: 'review-head' },
      r.rating ? stars(r.rating) : null,
      r.title && r.status === 'visible' ? userText('h3', r.title, { class: 'review-title' }) : null,
    ),
    h('p', { class: 'review-meta' },
      authorChip(r.author, r.author_name, { verified: r.author_verified, status: r.author_status }),
      h('span', { class: 'muted', text: ` ${fmt.date(r.ts)}` }),
    ),
  );
  if (purchaseText) {
    const tone = r.purchase === 'verified' ? 'good' : r.purchase === 'none' ? 'muted' : 'warn';
    li.append(h('p', { class: 'badges' }, badge(tone, purchaseText, 'receipt', SENTENCE.purchase[r.purchase] || null)));
  }
  const mod = moderationNotice(r, 'review', policy, client, r.id);
  if (mod) li.append(mod);
  if ((r.status === 'hidden' || r.status === 'retracted') && r.text != null) {
    const label = r.status === 'hidden' ? 'hidden' : 'withdrawn';
    const body = h('div', { class: 'review-text revealed', hidden: true }, r.title ? userText('h3', r.title, { class: 'review-title' }) : null, userText('p', r.text));
    li.append(
      h('button', {
        type: 'button',
        class: 'button small',
        'aria-expanded': 'false',
        on: {
          click: (ev) => {
            body.hidden = !body.hidden;
            ev.currentTarget.setAttribute('aria-expanded', String(!body.hidden));
            ev.currentTarget.textContent = body.hidden ? `Show the ${label} review` : 'Hide the review';
          },
        },
      }, `Show the ${label} review`),
      body,
    );
  } else if (r.text != null && r.status !== 'unavailable') {
    li.append(userText('p', r.text, { class: 'review-text' }));
  }
  if (!r.counted && r.excluded_reason && r.status === 'visible') {
    li.append(h('p', { class: 'hint excluded' }, `Not counted in the rating: ${EXCLUDED_TEXT[r.excluded_reason] || r.excluded_reason}.`));
  }
  if (r.status !== 'hidden') {
    const fs = flagSummary(r, 'review', client, r.id);
    if (fs) li.append(fs);
  }
  li.append(
    h('p', { class: 'review-foot' },
      recordLink(r.latest, 'Record'),
      ' ',
      h('a', { href: routes.publish('flag', { target: r.id }), class: 'quiet-link', text: 'Flag' }),
    ),
  );
  return li;
}

function detailsSection(p) {
  const sec = h('section', { class: 'block', 'aria-labelledby': 'h-details' }, h('h2', { id: 'h-details', text: 'Product details' }));
  const rows = [
    ['Name', factValue(p.name)],
    ['Brand', factValue(p.brand)],
    ['Category', p.category ? factValue(p.category, { render: (v) => h('span', { text: fmt.categoryText(v) }) }) : null],
    ['GTIN', p.gtin ? h('code', { text: fmt.printedGtin(p.gtin) }) : null],
    ['MPN', p.mpn ? bdi(p.mpn) : null],
    ['Product key', h('code', { class: 'hex', text: p.key })],
  ];
  sec.append(definitionList(rows));
  if (p.description) sec.append(h('div', { class: 'description' }, h('h3', { text: 'Description' }), factValue(p.description, { tag: 'p' })));
  const attrs = Object.entries(p.attributes || {});
  if (attrs.length) {
    sec.append(h('h3', { text: 'Attributes' }), definitionList(attrs.map(([k, f]) => [k, factValue(f)])));
  }
  if (p.images?.length) {
    const g = h('div', { class: 'images' });
    for (const img of p.images) g.append(productImage(img, p.name?.value));
    sec.append(h('h3', { text: 'Images' }), g);
  }
  const ex = Object.entries(p.excluded_sources || {}).filter(([, n]) => n > 0);
  if (ex.length) {
    const label = { hidden: 'hidden by moderation', retracted: 'withdrawn', unavailable: 'removed by a node', forked: "author's key has a conflicting history", revoked: 'after a key revocation' };
    sec.append(h('p', { class: 'hint', text: `Product records not used as sources: ${ex.map(([k, n]) => `${n} ${label[k] || k}`).join('; ')}.` }));
  }
  sec.append(h('p', { class: 'hint' }, 'Facts come from product records. When authors disagree, the value backed by the most identity-verified people is shown first; ties go to more people, then alphabetical order. ', h('a', { href: '#/data', text: 'Inspect records' }), '.'));
  void hex;
  return sec;
}
