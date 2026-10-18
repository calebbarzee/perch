import { h, bdi } from '../dom.js';
import * as fmt from '../format.js';
import { routes, emptyState, priceText, badge } from '../components.js';

const SORTS = [
  ['relevance', 'Relevance'],
  ['price', 'Lowest price'],
  ['rating_count', 'Most verified-identity reviews'],
  ['rating_mean', 'Highest rating'],
];

const SORT_TEXT = {
  relevance: 'Sorted by relevance.',
  price: 'Sorted by lowest price.',
  rating_count: 'Sorted by number of verified-identity reviews.',
  rating_mean: 'Sorted by rating, adjusted for review count.',
};

const FIELD_TEXT = { name: 'Name', brand: 'Brand', mpn: 'MPN', category: 'Category' };

// One `why` string (docs/views.md 11.4) as a sentence (docs/ux.md 8.1).
export function whySentence(w) {
  let m = /^gtin exact "(.*)" \+(\S+)$/.exec(w);
  if (m) return `GTIN matches: +${m[2]}`;
  m = /^(name|brand|mpn|category) (exact|prefix) "(.*)" \+(\S+)$/.exec(w);
  if (m) {
    const field = FIELD_TEXT[m[1]];
    return m[2] === 'exact' ? `${field} contains "${m[3]}": +${m[4]}` : `${field} has a word starting with "${m[3]}": +${m[4]}`;
  }
  m = /^rating (\S+) from (\d+) verified-identity reviews ([+-]\S+)$/.exec(w);
  if (m) {
    return Number(m[2]) === 0
      ? `No verified-identity reviews, so the neutral rating 3 is used: ${m[3]}`
      : `Rating ${m[1]} (adjusted for review count) from ${m[2]} verified-identity reviews: ${m[3]}`;
  }
  if (w === 'has price +5') return 'Has a current price: +5';
  if (w === 'no price +0') return 'No current price: +0';
  return w;
}

export default {
  id: 'search',
  live: (ctx) => JSON.stringify(ctx.client.stats()),
  render(ctx) {
    const { client, params } = ctx;
    const q = params.q || '';
    const sort = SORTS.some(([v]) => v === params.sort) ? params.sort : 'relevance';
    const currency = fmt.knownCurrency(params.currency) ? params.currency : null;
    const stats = client.stats();
    const hits = client.search(q, currency ? { sort, currency } : { sort });
    // Currencies present in the results, for the price sort.
    const resultCurrencies = [...new Set(hits.filter((x) => x.min_price).map((x) => x.min_price.currency))].sort();

    const form = h('form', { class: 'search-form', role: 'search' },
      h('div', { class: 'field grow' },
        h('label', { for: 'search-q', text: 'Search products' }),
        h('input', { id: 'search-q', name: 'q', type: 'search', value: q, autocomplete: 'off', spellcheck: 'false', placeholder: 'Name, brand, category, or GTIN' }),
      ),
      h('div', { class: 'field' },
        h('label', { for: 'search-sort', text: 'Sort by' }),
        h('select', { id: 'search-sort', name: 'sort' }, SORTS.map(([v, label]) => h('option', { value: v, selected: v === sort, text: label }))),
      ),
      resultCurrencies.length > 1 || currency
        ? h('div', { class: 'field' },
            h('label', { for: 'search-currency', text: 'Price currency' }),
            h('select', { id: 'search-currency', name: 'currency' },
              h('option', { value: '', text: 'Most common' }),
              [...new Set([...resultCurrencies, ...(currency ? [currency] : [])])].sort().map((c) => h('option', { value: c, selected: c === currency, text: c })),
            ),
          )
        : null,
      h('button', { type: 'submit', class: 'button primary', text: 'Search' }),
    );
    const submit = () => {
      let hash = routes.search(form.elements.q.value.trim(), form.elements.sort.value);
      const cur = form.elements.currency?.value;
      if (cur) hash += (hash.includes('?') ? '&' : '?') + `currency=${encodeURIComponent(cur)}`;
      location.hash = hash;
    };
    form.addEventListener('submit', (ev) => {
      ev.preventDefault();
      submit();
    });
    form.elements.sort.addEventListener('change', submit);
    form.elements.currency?.addEventListener('change', submit);

    const section = h('section', { class: 'screen screen-search' },
      h('h1', { text: q ? `Results for “${q}”` : 'Products' }),
      h('p', { class: 'lede' },
        `${fmt.plural(stats.products, 'product')} from ${fmt.plural(stats.records, 'signed record')} by ${fmt.plural(stats.authors, 'author')}, checked on this device. `,
        'Ratings count identity-verified reviewers only.',
      ),
      form,
    );

    if (!stats.records) {
      section.append(
        emptyState(
          'No records on this device yet.',
          'Connect to a node to sync the open dataset, or load a JSONL file.',
          h('p', { class: 'actions' }, h('a', { class: 'button primary', href: '#/network', text: 'Connect to a node' }), ' ', h('a', { class: 'button', href: '#/data', text: 'Load a file' })),
        ),
      );
      return section;
    }

    if (!hits.length) {
      section.append(emptyState(`No products match “${q}”.`, 'Try fewer words, a brand name, or the barcode number (GTIN).'));
      return section;
    }

    // Hits whose product record is hidden come last, collapsed (docs/ux.md 8.1).
    const visible = hits.filter((x) => (x.moderation_status ?? 'visible') === 'visible');
    const moderated = hits.filter((x) => (x.moderation_status ?? 'visible') !== 'visible');
    section.append(
      h('p', { class: 'result-count', role: 'status' },
        visible.length
          ? `${fmt.plural(visible.length, 'product')} ${visible.length === 1 ? 'matches' : 'match'}${q ? ` “${q}”` : ''}. ${SORT_TEXT[sort]}`
          : 'No visible products match.',
        /^\d{8}$|^\d{12,14}$/.test(q.trim()) ? ' Searching by GTIN.' : '',
      ),
      h('details', { class: 'computed' },
        h('summary', { text: 'How relevance works' }),
        h('p', { text: 'Each word of the query must match the product. Exact word matches score more than word starts: name 30 or 15, MPN 25 or 10, brand 20 or 10, category 10 or 5. A matching GTIN scores 100.' }),
        h('p', { text: 'The rating adds up to 20 points or takes up to 20 away. It is the mean of the identity-verified ratings with five neutral ratings of 1 to 5 added, so a few reviews move it less than many. A current price adds 5.' }),
        h('p', { text: 'Ranking uses only reviews from identity-verified people. No one can pay for position.' }),
      ),
    );
    const list = h('ol', { class: 'results', 'aria-label': 'Search results' });
    for (const hit of visible) list.append(resultItem(hit, sort));
    if (visible.length) section.append(list);
    if (moderated.length) {
      const group = h('details', { class: 'moderated-hits', open: !visible.length },
        h('summary', { text: `${fmt.plural(moderated.length, 'more product')} ${moderated.length === 1 ? 'matches' : 'match'}, but ${moderated.length === 1 ? 'its product record is' : 'their product records are'} hidden by moderation. Show them.` }),
        h('ol', { class: 'results' }, moderated.map((hit) => resultItem(hit, sort))),
      );
      section.append(group);
    }
    return section;
  },
};

function rawToggle(why) {
  const raw = h('pre', { class: 'json small', hidden: true, text: why.join('\n') });
  const btn = h('button', {
    type: 'button',
    class: 'link-button',
    'aria-expanded': 'false',
    on: {
      click: () => {
        raw.hidden = !raw.hidden;
        btn.setAttribute('aria-expanded', String(!raw.hidden));
        btn.textContent = raw.hidden ? 'Show raw' : 'Hide raw';
      },
    },
  }, 'Show raw');
  return h('div', {}, btn, raw);
}

function resultItem(hit, sort) {
  const rating =
    hit.rating_count > 0
      ? h('p', { class: 'hit-rating' },
          h('span', { class: 'hit-mean', text: fmt.mean(hit.rating_mean) }),
          h('span', { class: 'hit-of', text: ' / 5' }),
          h('span', { class: 'hit-count', text: ` from ${fmt.plural(hit.rating_count, 'verified-identity review')}` }),
        )
      : h('p', { class: 'hit-rating none', text: 'No verified-identity reviews' });
  const purchase = hit.verified_purchase_count
    ? h('p', { class: 'hit-purchase' }, badge('good', `${fmt.plural(hit.verified_purchase_count, 'verified purchase')}`, 'receipt'), hit.verified_purchase_mean != null ? ` mean ${fmt.mean(hit.verified_purchase_mean)}` : null)
    : null;
  return h('li', { class: 'hit', dataset: { key: hit.key } },
    h('div', { class: 'hit-main' },
      h('h2', { class: 'hit-name' }, h('a', { href: routes.product(hit.key) }, bdi(hit.name))),
      h('p', { class: 'hit-meta' },
        bdi(hit.brand),
        hit.category ? [h('span', { class: 'sep', 'aria-hidden': 'true', text: ' / ' }), h('span', { text: fmt.categoryText(hit.category) })] : null,
      ),
      hit.moderation_status && hit.moderation_status !== 'visible'
        ? h('p', { class: 'badges' }, badge('muted', hit.moderation_status === 'hidden' ? 'Hidden by moderation' : hit.moderation_status === 'retracted' ? 'Withdrawn' : 'Removed by node', 'eyeOff'))
        : null,
      h('details', { class: 'why' },
        h('summary', {}, 'Why this position'),
        h('ul', {}, hit.why.map((w) => h('li', {}, bdi(whySentence(w))))),
        h('p', { class: 'why-score', text: `Score: ${hit.score}` }),
        rawToggle(hit.why),
        sort === 'rating_mean' || sort === 'rating_count'
          ? h('p', { class: 'hint', text: `Ratings are sorted by a smoothed mean (${hit.rating_smoothed ?? '–'}): five neutral ratings are added, so a few reviews cannot outrank many.` })
          : sort !== 'relevance'
            ? h('p', { class: 'hint', text: 'Sorted by your choice, then by product key.' })
            : null,
      ),
    ),
    h('div', { class: 'hit-side' },
      rating,
      purchase,
      h('p', { class: 'hit-price' },
        hit.min_price ? [h('span', { class: 'muted', text: 'Lowest ' }), h('strong', { text: priceText(hit.min_price) })] : h('span', { class: 'muted', text: 'No current price' }),
      ),
    ),
  );
}
