import { h, bdi, userText } from '../dom.js';
import * as fmt from '../format.js';
import { emptyState, definitionList, authorChip, notice, badge, recordLink, urlLink, hex, routes, moderationNotice, identityBadge, SENTENCE, receiptShare } from '../components.js';
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
          h('h1', {}, a.name ? bdi(a.name) : a.profile?.status === 'hidden' ? 'Name hidden by moderation' : 'Unnamed author'),
          h('p', { class: 'muted' }, a.role ? `${fmt.ROLES[a.role] || a.role}. ` : '', 'Names are chosen by authors; the key identifies them.'),
        ),
      ),
    );
    const badges = h('p', { class: 'badges' });
    badges.append(identityBadge(a.verified));
    for (const d of a.domains || []) badges.append(badge('good', `Shop domain verified: ${d.domain}`, 'globe', SENTENCE.domain(d)));
    if (a.restricted) badges.append(badge('bad', 'Restricted seller', 'ban', SENTENCE.restricted(a, ctx.client.policy())));
    section.append(badges);
    if (ctx.client.author() === a.id) section.append(h('p', {}, 'This is your key. ', h('a', { href: '#/keys', text: 'Go to Keys' }), '.'));

    if (a.status === 'forked') {
      section.append(
        notice('bad', 'This author signed two different histories',
          h('p', { text: `Two signed records claim the same position in this author's log${a.fork_seq != null ? ` (record ${a.fork_seq})` : ''}. Views ignore the author's records from that point, and their reviews and flags do not count.` }),
          a.fork_evidence ? h('p', {}, 'Evidence: ', recordLink(a.fork_evidence[0]), ' and ', recordLink(a.fork_evidence[1]), '.') : null,
        ),
      );
    }
    if (a.status === 'revoked' && a.revoked) {
      section.append(notice('bad', 'Key reported stolen', h('p', {}, 'An attestor reported this key compromised. Records after number ', String(a.revoked.valid_through_seq), ' are ignored. Reported by ', authorChip(a.revoked.attestor, null), '.')));
    }
    if (a.status === 'retired') {
      section.append(
        notice('muted', 'Log retired',
          h('p', { text: `This author ended their log${a.retired ? ` at record ${a.retired.seq}` : ''}. Later records are ignored.` }),
          a.retired?.successor ? h('p', {}, 'The author names a successor key: ', authorChip(a.retired.successor, ctx.client.authorView(a.retired.successor)?.name ?? null), '. Verification and reputation do not transfer.') : null,
        ),
      );
    }
    if (a.restricted) {
      const r = a.restriction;
      section.append(
        notice('bad', 'Seller restricted',
          h('p', { text: r
            ? `${fmt.plural(r.hidden_listings, 'listing')} by this person are hidden, flagged by ${fmt.plural(r.distinct_flaggers, 'distinct person', 'distinct people')}. All their listings are marked.`
            : a.status === 'forked'
              ? 'This person signed two histories with this key. Every key of this person is restricted, and all their listings are marked.'
              : 'Another key of this person signed two histories. Every key of this person is restricted, and all their listings are marked.' }),
        ),
      );
    }
    if (a.profile && a.profile.status !== 'visible') {
      const pm = moderationNotice(a.profile, 'profile', ctx.client.policy(), ctx.client, null);
      if (pm) section.append(pm);
    }

    section.append(
      definitionList([
        ['Public key', hex(a.id, { n: 64 })],
        ['Website', a.url ? urlLink(a.url) : null],
        ['About', a.about ? userText('p', a.about) : null],
        ['Identity', a.verified
          ? h('span', {}, SENTENCE.identity(a.verified), ' Attestor: ', authorChip(a.verified.attestor, null))
          : h('span', { text: SENTENCE.notVerified })],
        ['Shop domains', (a.domains || []).length ? h('ul', { class: 'plain' }, a.domains.map((d) => h('li', {}, SENTENCE.domain(d), ' Attestor: ', authorChip(d.attestor, null)))) : null],
        ['Receipts', receiptShare(a) ?? undefined],
        ['Records', h('span', { text: fmt.number(a.record_count) })],
        ['Hidden listings', h('span', { text: fmt.number(a.hidden_listing_count) })],
      ]),
    );

    section.append(publishedSection(client, a.id));

    if (typeof client.authorLog === 'function') {
      const log = client.authorLog(a.id);
      const recent = log.slice(-50).reverse();
      section.append(
        h('details', { class: 'block log-details' },
          h('summary', {}, h('h2', { id: 'h-log', class: 'inline', text: `Signed log (${log.length} records)` })),
          h('p', { class: 'hint', text: 'Every record in order, including replaced versions, flags, and withdrawals.' }),
          h('div', { class: 'table-scroll' },
            h('table', { class: 'log-table' },
              h('thead', {}, h('tr', {}, h('th', { scope: 'col', class: 'num', text: 'Seq' }), h('th', { scope: 'col', text: 'Kind' }), h('th', { scope: 'col', text: 'Date' }), h('th', { scope: 'col', text: 'Record' }))),
              h('tbody', {}, recent.map((r) => h('tr', {}, h('td', { class: 'num', text: String(r.seq) }), h('td', {}, h('code', { text: r.kind })), h('td', { text: fmt.date(r.ts) }), h('td', {}, recordLink(client.verifyRecord(JSON.stringify(r)).id)))))),
          ),
          log.length > 50 ? h('p', { class: 'hint', text: 'Showing the latest 50 records.' }) : null,
        ),
      );
    }
    return section;
  },
};

const KIND_LABEL = {
  product: 'Product',
  listing: 'Listing',
  review: 'Review',
  observation: 'Price observation',
  flag: 'Flag',
  appeal: 'Appeal',
  profile: 'Profile',
  'attest.identity': 'Identity attestation',
  'attest.domain': 'Domain attestation',
  'attest.revoke': 'Key revocation',
  retract: 'Withdrawal',
  retire: 'Retirement',
  redact: 'Removal notice',
};

const STATUS_LABEL = {
  visible: null,
  hidden: ['muted', 'Hidden by moderation'],
  retracted: ['muted', 'Withdrawn'],
  unavailable: ['muted', 'Text removed'],
  invalid: ['warn', 'Ignored by views'],
};

function publishedSection(client, author) {
  const sec = h('section', { class: 'block', 'aria-labelledby': 'h-published' }, h('h2', { id: 'h-published', text: 'Published' }));
  if (typeof client.authorRecords !== 'function') return sec;
  const kinds = ['listing', 'review', 'product', 'flag', 'appeal', 'attest.identity', 'attest.domain', 'attest.revoke'];
  const items = client.authorRecords(author, { kinds, limit: 200 });
  if (!items.length) {
    sec.append(h('p', { class: 'muted', text: 'No listings, reviews, products, flags, or attestations.' }));
    return sec;
  }
  const list = h('ul', { class: 'plain chain-list' });
  for (const c of items) {
    const st = STATUS_LABEL[c.status];
    let target = routes.record(c.latest);
    if (c.kind === 'listing') target = routes.listing(c.root);
    else if ((c.kind === 'product' || c.kind === 'review') && c.product) target = routes.product(c.product);
    let label = c.title;
    if (!label && c.product) {
      const p = client.product(c.product);
      label = p?.name?.value ?? fmt.keyLabel(c.product);
    }
    let prefix = null;
    if (!label && (c.kind === 'flag' || c.kind === 'appeal' || c.kind.startsWith('attest.'))) {
      const body = client.record(c.latest)?.body;
      if (body?.subject) {
        label = client.authorView(body.subject)?.name ?? fmt.shortHex(body.subject);
        prefix = 'about ';
        target = routes.author(body.subject);
      } else if (body?.target) {
        const t = client.record(body.target);
        const tb = t?.body || {};
        label = tb.title || tb.name || (tb.product ? client.product(tb.product)?.name?.value : null) || (t ? `${t.kind} ${fmt.shortHex(body.target)}` : fmt.shortHex(body.target));
        prefix = c.kind === 'flag' ? `${fmt.FLAG_REASONS[body.reason] || body.reason}: ` : 'on ';
        target = routes.record(body.target);
      }
    }
    list.append(
      h('li', { class: 'chain', dataset: { kind: c.kind, status: c.status } },
        h('span', { class: 'chain-kind', text: KIND_LABEL[c.kind] || c.kind }),
        prefix ? h('span', { class: 'muted', text: prefix }) : null,
        h('a', { href: target }, label ? bdi(label) : hex(c.root)),
        st ? badge(st[0], st[1]) : null,
        h('span', { class: 'muted', text: fmt.date(c.ts) }),
      ),
    );
  }
  sec.append(list);
  if (items.length === 200) sec.append(h('p', { class: 'hint', text: 'Showing the latest 200.' }));
  return sec;
}
