import { h, bdi, userText } from '../dom.js';
import * as fmt from '../format.js';
import { emptyState, definitionList, authorChip, notice, badge, recordLink, urlLink, hex, routes } from '../components.js';
import { fingerprint } from '../fingerprint.js';

export default {
  id: 'author',
  nav: 'search',
  live: (ctx) => JSON.stringify(ctx.client.authorView(ctx.args.id)),
  render(ctx) {
    const { client, args } = ctx;
    const a = client.authorView(args.id);
    if (!a) {
      return h('section', { class: 'screen' },
        h('h1', { text: 'Author not found' }),
        emptyState(`No records by ${fmt.shortHex(args.id)} are in your local data.`, 'Sync with more nodes on the Network screen.'),
      );
    }
    const section = h('article', { class: 'screen screen-author', dataset: { author: a.id } },
      h('div', { class: 'author-head' },
        fingerprint(a.id, 48),
        h('div', {},
          h('h1', {}, a.name ? bdi(a.name) : 'Author without a profile'),
          h('p', { class: 'muted' }, a.role ? `${fmt.ROLES[a.role] || a.role}. ` : '', 'Names are chosen by authors; the key identifies them.'),
        ),
      ),
    );
    const badges = h('p', { class: 'badges' });
    if (a.verified) badges.append(badge('good', `Identity verified (${a.verified.level})`, 'shield'));
    else badges.append(badge('muted', 'Identity not verified', 'shield'));
    for (const d of a.domains || []) badges.append(badge('good', `Domain ${d.domain}`, 'globe'));
    if (a.restricted) badges.append(badge('bad', 'Restricted', 'ban'));
    section.append(badges);

    if (a.status === 'forked') {
      section.append(notice('bad', 'This author signed two different histories', h('p', { text: `Two records claim the same position in this author's log${a.fork_seq != null ? ` (from record ${a.fork_seq})` : ''}. Views ignore the author's records from that point, and their reviews and flags do not count.` })));
    }
    if (a.status === 'revoked' && a.revoked) {
      section.append(notice('bad', 'Key reported stolen', h('p', {}, 'An attestor reported this key compromised. Records after number ', String(a.revoked.valid_through_seq), ' are ignored. Reported by ', authorChip(a.revoked.attestor, null), '.')));
    }
    if (a.status === 'retired') {
      section.append(notice('muted', 'Log retired', h('p', { text: 'This author ended their log. Later records are ignored.' })));
    }
    if (a.restricted) {
      section.append(notice('bad', 'Seller restricted', h('p', { text: `${fmt.plural(a.hidden_listing_count, 'listing')} by this person are hidden by moderation. All their listings are marked.` })));
    }

    section.append(
      definitionList([
        ['Public key', hex(a.id, { n: 64 })],
        ['Website', a.url ? urlLink(a.url) : null],
        ['About', a.about ? userText('p', a.about) : null],
        ['Identity', a.verified ? h('span', {}, `${a.verified.level === 'business' ? 'Business' : 'Person'}${a.verified.country ? ` in ${a.verified.country}` : ''}, attested by `, authorChip(a.verified.attestor, null), ` until ${fmt.date(a.verified.expires)}`) : h('span', { text: 'No counting attestation under your policy.' })],
        ['Domains', (a.domains || []).length ? h('ul', { class: 'plain' }, a.domains.map((d) => h('li', {}, h('span', { class: 'host', text: d.domain }), ', attested by ', authorChip(d.attestor, null), ` until ${fmt.date(d.expires)}`))) : null],
        ['Records', h('span', { text: fmt.number(a.record_count) })],
        ['Hidden listings', h('span', { text: fmt.number(a.hidden_listing_count) })],
      ]),
    );

    if (typeof client.authorLog === 'function') {
      const log = client.authorLog(a.id);
      const recent = log.slice(-50).reverse();
      section.append(
        h('section', { class: 'block', 'aria-labelledby': 'h-log' },
          h('h2', { id: 'h-log', text: `Log (${log.length} records)` }),
          h('div', { class: 'table-scroll' },
            h('table', { class: 'log-table' },
              h('thead', {}, h('tr', {}, h('th', { scope: 'col', class: 'num', text: 'Seq' }), h('th', { scope: 'col', text: 'Kind' }), h('th', { scope: 'col', text: 'Date' }), h('th', { scope: 'col', text: 'Record' }))),
              h('tbody', {}, recent.map((r) => h('tr', {}, h('td', { class: 'num', text: String(r.seq) }), h('td', {}, h('code', { text: r.kind })), h('td', { text: fmt.date(r.ts) }), h('td', {}, recordLink(client.verifyRecord(JSON.stringify(r)).id)))))),
          ),
          log.length > 50 ? h('p', { class: 'hint', text: 'Showing the latest 50 records.' }) : null,
        ),
      );
    }
    section.append(h('p', { class: 'actions' }, h('a', { class: 'button', href: routes.publish('flag', {}), text: 'Flag a record' })));
    return section;
  },
};
