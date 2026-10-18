import { h, bdi, userText } from '../dom.js';
import * as fmt from '../format.js';
import {
  routes, authorChip, badge, notice, stars, moderationNotice, factValue, productImage,
  priceText, emptyState, definitionList, PURCHASE_TEXT, EXCLUDED_TEXT, icon, hex, recordLink, urlLink,
} from '../components.js';
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
    const section = h('article', { class: 'screen screen-product', dataset: { key: p.key } });

    // Header.
    section.append(
      h('header', { class: 'product-head' },
        h('p', { class: 'crumbs' }, h('a', { href: routes.search(p.brand?.value || ''), text: 'Search' }), p.category ? [' / ', h('span', { text: fmt.categoryText(p.category.value) })] : null),
        h('h1', {}, bdi(p.name?.value ?? p.key)),
        h('p', { class: 'product-sub' }, h('span', { class: 'brand' }, bdi(p.brand?.value ?? '')), ' ', h('span', { class: 'key', text: fmt.keyLabel(p.key) })),
      ),
    );

    // Summary: rating and price side by side.
    section.append(h('div', { class: 'summary-grid' }, ratingPanel(p.ratings), pricePanel(p)));

    // Listings.
    section.append(listingsSection(p, policy));

    // Price history.
    const currencies = [...new Set(p.listings.flatMap((l) => l.price_history.map((x) => x.currency)))].sort();
    if (currencies.length) {
      const charts = h('section', { class: 'block', 'aria-labelledby': 'h-history' }, h('h2', { id: 'h-history', text: 'Price history' }));
      const nowTs = policy.now ?? Date.now();
      for (const c of currencies) charts.append(priceChart({ listings: p.listings.filter((l) => l.status !== 'unavailable'), currency: c, now: nowTs, title: currencies.length > 1 ? `Prices in ${c}` : `Prices in ${c}` }));
      section.append(charts);
    }

    // Reviews.
    section.append(reviewsSection(p, policy));

    // Details: facts with sources.
    section.append(detailsSection(p));

    return section;
  },
};

function histogram(hist, label) {
  const max = Math.max(1, ...hist);
  const total = hist.reduce((a, b) => a + b, 0);
  return h('table', { class: 'histogram' },
    h('caption', { class: 'visually-hidden', text: label }),
    h('tbody', {},
      [5, 4, 3, 2, 1].map((star) => {
        const n = hist[star - 1];
        return h('tr', {},
          h('th', { scope: 'row', text: `${star} star${star === 1 ? '' : 's'}` }),
          h('td', { class: 'bar-cell' }, h('span', { class: 'bar', style: null, dataset: { w: String(Math.round((n / max) * 100)) } })),
          h('td', { class: 'num', text: total ? `${n}` : '0' }),
        );
      }),
    ),
  );
}

function sizeBars(root) {
  for (const b of root.querySelectorAll('.bar[data-w]')) b.style.width = `${b.dataset.w}%`;
}

function ratingPanel(r) {
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
    panel.append(h('p', { class: 'headline-rating none', text: 'No ratings from identity-verified reviewers yet.' }));
  }
  const hist = histogram(vi.histogram, 'Ratings from identity-verified reviewers');
  sizeBars(hist);
  panel.append(hist);
  panel.append(
    h('dl', { class: 'secondary-ratings' },
      h('div', { dataset: { testid: 'rating-purchase' } },
        h('dt', {}, icon('receipt'), ' Verified purchases'),
        h('dd', { text: vp.count ? `${fmt.mean(vp.mean)} from ${fmt.plural(vp.count, 'review')}` : 'None' }),
      ),
      h('div', { dataset: { testid: 'rating-all' } },
        h('dt', { text: 'All counted reviews' }),
        h('dd', { text: r.count ? `${fmt.mean(r.mean)} from ${fmt.plural(r.count, 'review')}` : 'None' }),
      ),
    ),
  );
  const ex = Object.entries(r.excluded || {}).filter(([, n]) => n > 0);
  if (ex.length) {
    panel.append(
      h('p', { class: 'hint excluded-summary' },
        `Not counted: ${ex.map(([k, n]) => `${n} ${EXCLUDED_TEXT[k] || k}`).join('; ')}.`,
      ),
    );
  }
  panel.append(h('p', { class: 'hint', text: 'The headline counts each identity-verified person once, so new keys cannot change it.' }));
  return panel;
}

function pricePanel(p) {
  const panel = h('section', { class: 'panel price-panel', 'aria-labelledby': 'h-price' }, h('h2', { id: 'h-price', text: 'Price' }));
  const entries = Object.entries(p.price_range).sort((a, b) => b[1].count - a[1].count || (a[0] < b[0] ? -1 : 1));
  if (!entries.length) {
    panel.append(h('p', { class: 'muted', text: 'No visible listings.' }));
    return panel;
  }
  for (const [cur, r] of entries) {
    const row = h('div', { class: 'price-range', dataset: { currency: cur } });
    if (r.count) {
      row.append(
        h('p', { class: 'range' },
          h('strong', { class: 'big', text: fmt.money(r.min, cur) }),
          r.max !== r.min ? [h('span', { class: 'to', text: ' to ' }), h('strong', { text: fmt.money(r.max, cur) })] : null,
        ),
        h('p', { class: 'headline-sub', text: `Median ${fmt.money(r.median, cur)} across ${fmt.plural(r.count, 'listing')} on verified seller domains` }),
      );
    } else {
      row.append(h('p', { class: 'muted', text: `No ${cur} listings on verified seller domains.` }));
    }
    if (r.excluded_unverified_domain) {
      row.append(h('p', { class: 'hint', text: `${fmt.plural(r.excluded_unverified_domain, 'listing')} not in the range: the seller's domain is not verified.` }));
    }
    panel.append(row);
  }
  panel.append(h('p', { class: 'hint', text: `${fmt.plural(p.listings.length, 'listing')} in total, including hidden ones.` }));
  return panel;
}

function listingsSection(p, policy) {
  const sec = h('section', { class: 'block', 'aria-labelledby': 'h-listings' }, h('h2', { id: 'h-listings', text: `Where to buy (${p.listings.length})` }));
  if (!p.listings.length) {
    sec.append(h('p', { class: 'muted', text: 'No seller has listed this product.' }));
    return sec;
  }
  const list = h('ul', { class: 'listings' });
  for (const l of p.listings) list.append(listingCard(l, policy));
  sec.append(list);
  return sec;
}

export function listingCard(l, policy, { showProduct = false } = {}) {
  const li = h('li', { class: ['listing', `status-${l.status}`], dataset: { listing: l.id, status: l.status } });
  const seller = authorChip(l.seller, l.seller_name, { verified: l.seller_verified, status: l.seller_status });
  const badges = h('p', { class: 'badges' },
    l.domain_verified
      ? badge('good', `Domain verified: ${l.domain?.domain ?? fmt.host(l.url)}`, 'globe')
      : badge('warn', 'Domain not verified', 'globe'),
    l.seller_restricted ? badge('bad', 'Seller restricted', 'ban') : null,
    l.flags.count ? badge('muted', `${fmt.plural(l.flags.count, 'flag')}`, 'flag') : null,
  );
  const mod = moderationNotice(l, 'listing', policy);
  const head = h('div', { class: 'listing-head' },
    h('div', { class: 'listing-who' }, seller, l.title ? userText('p', l.title, { class: 'listing-title' }) : null),
    h('div', { class: 'listing-price' },
      h('p', { class: 'price', text: priceText(l.price) }),
      h('p', { class: 'muted' }, `${fmt.AVAILABILITY[l.availability] || l.availability || ''}, ${fmt.CONDITION[l.condition] || l.condition || ''}`),
    ),
  );
  li.append(head, badges);
  if (l.seller_restricted) {
    li.append(notice('bad', 'Seller restricted', h('p', { text: `People flagged at least ${policy.seller_hidden_listing_limit} of this seller's listings until they were hidden.` })));
  }
  if (mod) li.append(mod);
  li.append(
    h('p', { class: 'listing-links' },
      l.url && l.status !== 'unavailable' ? urlLink(l.url, 'Seller page') : null,
      ' ',
      h('a', { href: routes.listing(l.id), text: 'Listing details' }),
      showProduct ? [' ', h('a', { href: routes.product(l.product), text: 'Product' })] : null,
      h('span', { class: 'muted', text: ` Updated ${fmt.date(l.updated_ts)}` }),
    ),
  );
  return li;
}

function reviewsSection(p, policy) {
  const sec = h('section', { class: 'block', 'aria-labelledby': 'h-reviews' },
    h('div', { class: 'block-head' },
      h('h2', { id: 'h-reviews', text: `Reviews (${p.reviews.length})` }),
      h('a', { class: 'button', href: routes.publish('review', { product: p.key }), text: 'Write a review' }),
    ),
  );
  if (!p.reviews.length) {
    sec.append(h('p', { class: 'muted', text: 'No reviews yet.' }));
    return sec;
  }
  const filters = h('div', { class: 'review-filter', role: 'group', 'aria-label': 'Show reviews' });
  const list = h('ul', { class: 'reviews' });
  const opts = [
    ['all', 'All'],
    ['identity', 'Identity verified'],
    ['purchase', 'Verified purchase'],
    ['excluded', 'Not counted'],
  ];
  const apply = (which) => {
    for (const b of filters.querySelectorAll('button')) b.setAttribute('aria-pressed', String(b.dataset.filter === which));
    for (const li of list.children) {
      const d = li.dataset;
      const show =
        which === 'all' ||
        (which === 'identity' && d.identity === 'true') ||
        (which === 'purchase' && d.purchase === 'verified') ||
        (which === 'excluded' && d.counted === 'false');
      li.hidden = !show;
    }
  };
  for (const [v, label] of opts) {
    const n = p.reviews.filter((r) =>
      v === 'all' ? true : v === 'identity' ? r.author_verified : v === 'purchase' ? r.verified_purchase : !r.counted,
    ).length;
    filters.append(h('button', { type: 'button', class: 'chip', dataset: { filter: v }, 'aria-pressed': String(v === 'all'), on: { click: () => apply(v) } }, `${label} (${n})`));
  }
  for (const r of p.reviews) list.append(reviewItem(r, policy));
  sec.append(filters, list);
  return sec;
}

export function reviewItem(r, policy) {
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
    li.append(h('p', { class: 'badges' }, r.purchase === 'verified' ? badge('good', purchaseText, 'receipt') : badge('warn', purchaseText, 'receipt')));
  }
  const mod = moderationNotice(r, 'review', policy);
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
  li.append(
    h('p', { class: 'review-foot' },
      r.flags.count ? h('span', { class: 'muted', text: `${fmt.plural(r.flags.count, 'flag')} (${fmt.reasonsText(r.flags.reasons)}). ` }) : null,
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
    ['GTIN', p.gtin ? h('code', { text: p.gtin }) : null],
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
  sec.append(h('p', { class: 'hint' }, 'Facts come from product records. When authors disagree, the value with the most identity-verified sources is shown first. ', h('a', { href: '#/data', text: 'Inspect records' }), '.'));
  void hex;
  return sec;
}
