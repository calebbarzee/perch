import { h, field, announce, copyText } from '../dom.js';
import { checksumNote } from '../app.js';
import { APP_VERSION } from '../client.js';
import * as fmt from '../format.js';
import { notice } from '../components.js';
import { configuredNodes, saveSettings, urlNodes, validNodeUrl, defaultNodes } from '../settings.js';
import { httpBaseFor } from '../client.js';

const STATE_TEXT = {
  connecting: 'Connecting',
  syncing: 'Syncing',
  live: 'Connected',
  backoff: 'Reconnecting',
  closed: 'Disconnected',
  error: 'Error',
};

export default {
  id: 'network',
  live: (ctx) => JSON.stringify(ctx.client.peers()),
  render(ctx) {
    const { client } = ctx;
    const peers = client.peers();
    const nodes = configuredNodes();
    const section = h('section', { class: 'screen screen-network' }, h('h1', { text: 'Network' }));
    section.append(
      h('p', { class: 'lede', text: 'This device syncs signed records from the nodes below and checks every record itself. A node can withhold records, so connecting to several nodes and comparing them protects you from a node that hides data.' }),
    );

    const live = peers.filter((p) => p.state === 'live');
    if (nodes.length < 3) {
      section.append(notice('info', `Connected to ${fmt.plural(nodes.length, 'node')}`, h('p', { text: 'Add at least 3 independent nodes so one node cannot hide records from you unnoticed.' })));
    }
    const pubStatus = typeof client.publishStatus === 'function' ? client.publishStatus() : null;
    const unproven = (pubStatus?.nodes || []).filter((n) => n.unproven);
    if (unproven.length) {
      section.append(
        notice('warn', 'A node claims records it cannot prove',
          h('p', { text: `${unproven.map((n) => n.url).join(', ')} ${unproven.length === 1 ? 'claims' : 'claim'} a newer head for your log but did not send the signed records. The claim is ignored.` }),
        ),
      );
    }
    const disagreeing = live.filter((p) => p.behind || p.ahead);
    if (disagreeing.length) {
      section.append(
        notice('warn', 'Nodes disagree', h('p', { text: `${fmt.plural(disagreeing.length, 'node')} ${disagreeing.length === 1 ? 'holds' : 'hold'} different log heads than this device. A node that is behind may be slow, or may be hiding records. Records that this device has are sent to nodes that lack them.` })),
      );
    } else if (live.length >= 2) {
      section.append(notice('good', 'Nodes agree', h('p', { text: `${fmt.plural(live.length, 'connected node')} report the same log heads as this device.` })));
    }

    const table = h('table', { class: 'peers', id: 'peers-table' },
      h('caption', { class: 'visually-hidden', text: 'Connected nodes' }),
      h('thead', {}, h('tr', {},
        h('th', { scope: 'col', text: 'Node' }),
        h('th', { scope: 'col', text: 'State' }),
        h('th', { scope: 'col', class: 'num', text: 'Received' }),
        h('th', { scope: 'col', class: 'num', text: 'Sent' }),
        h('th', { scope: 'col', class: 'num', text: 'Behind' }),
        h('th', { scope: 'col', class: 'num', text: 'Ahead' }),
        h('th', { scope: 'col' }, h('span', { class: 'visually-hidden', text: 'Actions' })),
      )),
      h('tbody', {},
        peers.map((p) =>
          h('tr', { dataset: { url: p.url, state: p.state } },
            h('td', {}, h('code', { text: p.url })),
            h('td', {}, h('span', { class: `state state-${p.state}`, text: STATE_TEXT[p.state] || p.state })),
            h('td', { class: 'num', text: fmt.number(p.received) }),
            h('td', { class: 'num', text: fmt.number(p.sent) }),
            h('td', { class: ['num', p.behind ? 'warn-text' : null], text: fmt.number(p.behind) }),
            h('td', { class: ['num', p.ahead ? 'warn-text' : null], text: fmt.number(p.ahead) }),
            h('td', {},
              h('button', {
                type: 'button',
                class: 'button small',
                on: {
                  click: async () => {
                    await client.disconnect(p.url);
                    saveSettings({ nodes: configuredNodes().filter((u) => u !== p.url) });
                    announce(`Removed ${p.url}`);
                    ctx.refresh();
                  },
                },
              }, 'Remove'),
              ' ',
              httpBaseFor(p.url) ? h('a', { href: `${httpBaseFor(p.url)}/v1/status`, target: '_blank', rel: 'noopener noreferrer', class: 'quiet-link', text: 'Status' }) : null,
            ),
          ),
        ),
      ),
    );
    section.append(
      h('section', { class: 'block' },
        h('h2', { text: 'Nodes' }),
        peers.length ? h('div', { class: 'table-scroll' }, table) : h('p', { class: 'muted', text: 'No nodes configured.' }),
        h('p', { class: 'hint', text: 'Behind: authors whose latest record this device has but the node lacks. Ahead: the reverse, until sync finishes.' }),
      ),
    );

    const input = h('input', { id: 'node-url', name: 'url', type: 'url', placeholder: 'ws://127.0.0.1:7700/v1/sync', spellcheck: 'false', autocomplete: 'off' });
    const status = h('div', { role: 'status' });
    const form = h('form', { class: 'panel', id: 'add-node' },
      h('h2', { text: 'Add a node' }),
      field({ id: 'node-url', label: 'Node sync address', hint: 'WebSocket URL ending in /v1/sync', control: input }),
      h('button', { type: 'submit', class: 'button primary', text: 'Connect' }),
      status,
    );
    form.addEventListener('submit', async (ev) => {
      ev.preventDefault();
      let url = input.value.trim();
      if (/^https?:\/\//.test(url)) url = url.replace(/^http/, 'ws').replace(/\/?$/, '/v1/sync').replace(/\/v1\/sync\/v1\/sync$/, '/v1/sync');
      if (!validNodeUrl(url)) {
        status.replaceChildren(notice('bad', 'Not a node address', h('p', { text: 'Use a ws:// or wss:// URL, for example ws://127.0.0.1:7700/v1/sync.' })));
        input.focus();
        return;
      }
      const list = configuredNodes();
      if (!list.includes(url)) saveSettings({ nodes: [...list, url] });
      client.connect(url).catch((e) => status.replaceChildren(notice('bad', 'Connection failed', h('p', { text: String(e.message || e) }))));
      announce(`Connecting to ${url}`);
      ctx.refresh();
    });
    section.append(form);

    // This device and the running build (docs/ux.md 8.7 items 4 and 5).
    const stats = client.stats();
    section.append(
      h('section', { class: 'panel', 'aria-labelledby': 'h-device' },
        h('h2', { id: 'h-device', text: 'This device' }),
        h('p', { text: `${fmt.plural(stats.records, 'record')} from ${fmt.plural(stats.authors, 'author')}: ${fmt.plural(stats.products, 'product')}, ${fmt.plural(stats.listings, 'listing')}, ${fmt.plural(stats.reviews, 'review')}. ${fmt.plural(stats.forks, 'conflicting history', 'conflicting histories')}, ${fmt.plural(stats.invalid, 'invalid record')}, ${fmt.plural(stats.redacted, 'removed body', 'removed bodies')}.` }),
      ),
    );
    const wasm = ctx.app.build?.wasm_sha256;
    const grouped = wasm ? wasm.match(/.{1,16}/g).join('\u2009') : null;
    section.append(
      h('section', { class: 'panel', 'aria-labelledby': 'h-build' },
        h('h2', { id: 'h-build', text: 'Build' }),
        h('dl', { class: 'facts' },
          h('div', { class: 'fact-row' }, h('dt', { text: 'App version' }), h('dd', { text: `${APP_VERSION}${ctx.app.mode === 'mock' ? ' (mock client)' : ''}` })),
          h('div', { class: 'fact-row' }, h('dt', { text: 'Wasm SHA-256' }), h('dd', {},
            wasm ? [h('code', { class: 'hex hex-full', id: 'net-wasm-sha', text: grouped }), ' ', h('button', { type: 'button', class: 'button small', on: { click: () => copyText(wasm) } }, 'Copy')] : h('span', { text: 'none (mock client)' }),
          )),
          h('div', { class: 'fact-row' }, h('dt', { text: 'Policy hash' }), h('dd', {}, h('code', { class: 'hex hex-full', text: client.policyHash() }))),
        ),
        wasm ? h('p', {}, 'The wasm module ', checksumNote(false)) : null,
      ),
    );

    if (urlNodes()) {
      section.append(h('p', { class: 'hint', text: 'The page address sets the node list (?nodes=). Changes here apply after you remove that part of the address.' }));
    }
    section.append(
      h('p', {},
        h('button', {
          type: 'button',
          class: 'button',
          on: {
            click: () => {
              saveSettings({ nodes: null });
              announce('Node list reset.');
              location.reload();
            },
          },
        }, 'Reset to default nodes'),
        h('span', { class: 'hint', text: ` Default: ${defaultNodes().join(', ') || 'none'}` }),
      ),
    );
    return section;
  },
};
