import { h, bdi } from '../dom.js';
import * as fmt from '../format.js';
import { routes, emptyState, priceText, badge } from '../components.js';

const SORTS = [
  ['relevance', 'Best match'],
  ['rating_mean', 'Highest verified rating'],
  ['rating_count', 'Most verified ratings'],
  ['price', 'Lowest price'],
];

export default {
  id: 'search',
  live: (ctx) => JSON.stringify(ctx.client.stats()),
  render(ctx) {
    const { client, params } = ctx;
    const q = params.q || '';
    const sort = SORTS.some(([v]) => v === params.sort) ? params.sort : 'relevance';
    const stats = client.stats();
    const hits = client.search(q, { sort });

    const form = h('form', { class: 'search-form', role: 'search' },
      h('div', { class: 'field grow' },
        h('label', { for: 'search-q', text: 'Search products' }),
        h('input', { id: 'search-q', name: 'q', type: 'search', value: q, autocomplete: 'off', spellcheck: 'false', placeholder: 'Name, brand, category, or GTIN' }),
      ),
      h('div', { class: 'field' },
        h('label', { for: 'search-sort', text: 'Sort by' }),
        h('select', { id: 'search-sort', name: 'sort' }, SORTS.map(([v, label]) => h('option', { value: v, selected: v === sort, text: label }))),
      ),
      h('button', { type: 'submit', class: 'button primary', text: 'Search' }),
    );
    form.addEventListener('submit', (ev) => {
      ev.preventDefault();
      location.hash = routes.search(form.elements.q.value.trim(), form.elements.sort.value);
    });
    form.elements.sort.addEventListener('change', () => {
      location.hash = routes.search(form.elements.q.value.trim(), form.elements.sort.value);
    });

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

    section.append(h('p', { class: 'result-count', role: 'status', text: fmt.plural(hits.length, 'result') }));
    const list = h('ol', { class: 'results', 'aria-label': 'Search results' });
    for (const hit of hits) list.append(resultItem(hit, sort));
    section.append(list);
    return section;
  },
};

function resultItem(hit, sort) {
  const rating =
    hit.rating_count > 0
      ? h('p', { class: 'hit-rating' },
          h('span', { class: 'hit-mean', text: fmt.mean(hit.rating_mean) }),
          h('span', { class: 'hit-of', text: ' / 5' }),
          h('span', { class: 'hit-count', text: ` from ${fmt.plural(hit.rating_count, 'verified reviewer')}` }),
        )
      : h('p', { class: 'hit-rating none', text: 'No ratings from verified reviewers' });
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
      h('details', { class: 'why' },
        h('summary', {}, `Why this result (score ${hit.score})`),
        h('ul', {}, hit.why.map((w) => h('li', {}, bdi(w)))),
        sort !== 'relevance' ? h('p', { class: 'hint', text: 'Sorted by your choice, then by product key.' }) : null,
      ),
    ),
    h('div', { class: 'hit-side' },
      rating,
      purchase,
      h('p', { class: 'hit-price' },
        hit.min_price ? [h('span', { class: 'muted', text: 'From ' }), h('strong', { text: priceText(hit.min_price) })] : h('span', { class: 'muted', text: 'No verified price' }),
      ),
    ),
  );
}
