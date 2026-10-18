import { h, bdi } from '../dom.js';
import * as fmt from '../format.js';
import { routes, emptyState, definitionList, authorChip, recordLink, urlLink, hex, receiptShare, SENTENCE, PRICE_EXCLUDED_TEXT } from '../components.js';
import { listingCard } from './product.js';
import { priceChart } from '../chart.js';

export default {
  id: 'listing',
  nav: 'search',
  live: (ctx) => JSON.stringify(ctx.client.listing(ctx.args.id)),
  render(ctx) {
    const { client, args } = ctx;
    const l = client.listing(args.id);
    if (!l) {
      return h('section', { class: 'screen' },
        h('h1', { text: 'Listing not found' }),
        emptyState(`No listing with ID ${fmt.shortHex(args.id)} is in your local data.`, 'Listings are found by the ID of their first version.', h('a', { class: 'button', href: '#/search', text: 'Search' })),
      );
    }
    const policy = client.policy();
    const product = client.product(l.product);
    const productName = product?.name?.value ?? fmt.keyLabel(l.product);
    const section = h('article', { class: 'screen screen-listing', dataset: { listing: l.id } },
      h('p', { class: 'crumbs' }, h('a', { href: routes.product(l.product) }, bdi(productName)), ' / listing'),
      // A hidden listing's title is collapsed with its body (M1).
      h('h1', {}, l.status === 'hidden' ? ['Hidden listing by ', bdi(l.seller_name || fmt.shortHex(l.seller))] : l.title ? bdi(l.title) : ['Listing by ', bdi(l.seller_name || fmt.shortHex(l.seller))]),
    );
    section.append(h('ul', { class: 'listings single' }, listingCard(l, policy, { client })));

    section.append(
      h('section', { class: 'block', 'aria-labelledby': 'h-ldetails' },
        h('h2', { id: 'h-ldetails', text: 'Details' }),
        definitionList([
          ['Product', h('a', { href: routes.product(l.product) }, bdi(productName))],
          ['Seller', authorChip(l.seller, l.seller_name, { verified: l.seller_verified, status: l.seller_status })],
          ['Seller page', l.status === 'hidden' ? h('span', { class: 'muted', text: 'Collapsed with the hidden listing above.' }) : l.url ? urlLink(l.url) : null],
          ['Ships to', l.ships_to?.length ? h('span', { text: l.ships_to.join(', ') }) : null],
          ['Ships from', l.location ? h('span', { text: `${l.location.country}${l.location.region ? `, ${l.location.region}` : ''}` }) : null],
          ['Price summary', h('span', { text: l.feeds_prices ? 'Included' : `Not included: ${PRICE_EXCLUDED_TEXT[l.price_excluded_reason] || l.price_excluded_reason}` })],
          ['Shop domain', l.domain_verified && l.domain ? h('span', { text: SENTENCE.domain(l.domain) }) : h('span', { class: 'warn-text', text: SENTENCE.domainNot(fmt.host(l.url)) })],
          ['Status', h('span', { text: { visible: 'Visible', hidden: 'Hidden by moderation', retracted: 'Withdrawn by the seller', unavailable: 'Text removed by a node' }[l.status] || l.status })],
          ['Flags', h('span', { text: l.flags.count ? `${l.flags.count} in total, ${l.flags.counted} counted (${fmt.reasonsText(l.flags.reasons)})` : 'No flags' })],
          ['Listing ID', hex(l.id, { n: 64 })],
          ['Latest version', recordLink(l.latest)],
          ['Updated', h('span', { text: fmt.dateTime(l.updated_ts) })],
        ]),
      ),
    );

    const share = receiptShare(client.authorView(l.seller));
    if (share) section.append(h('section', { class: 'block', 'aria-labelledby': 'h-receipts' }, h('h2', { id: 'h-receipts', text: 'Receipts from this seller' }), share));

    if (l.price_history.length && l.price) {
      const cur = l.price.currency;
      const chart = priceChart({ listings: [l], currency: cur, now: policy.now ?? Date.now(), title: `Prices in ${cur}`, pick: true });
      section.append(
        h('section', { class: 'block', 'aria-labelledby': 'h-lhistory' },
          h('h2', { id: 'h-lhistory', text: 'Price history' }),
          l.status === 'hidden' ? h('details', {}, h('summary', { text: 'Show the price history of the hidden listing' }), chart) : chart,
        ),
      );
    }

    const actions = h('p', { class: 'actions' },
      h('a', { class: 'button', href: routes.publish('flag', { target: l.id }), text: 'Flag this listing' }),
    );
    const me = client.author();
    if (me && me === l.seller) {
      actions.append(' ',
        h('a', { class: 'button', href: routes.publish('listing', { replaces: l.latest, root: l.id }), text: 'Update price' }),
        ' ',
        h('a', { class: 'button', href: routes.publish('receipt', { listing: l.id }), text: 'Issue a receipt' }),
        ' ',
        l.status === 'hidden' ? h('a', { class: 'button', href: routes.publish('appeal', { target: l.id }), text: 'Appeal' }) : null,
      );
    }
    section.append(actions);
    return section;
  },
};
