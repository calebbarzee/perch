import { h, field, announce, downloadText, userText, copyText } from '../dom.js';
import * as fmt from '../format.js';
import { notice, hex, authorChip, recordLink, routes, definitionList, ignoredText } from '../components.js';
import { APP_VERSION } from '../client.js';

export default {
  id: 'data',
  live: (ctx) => (ctx.args.id ? JSON.stringify(ctx.client.record(ctx.args.id)) : JSON.stringify(ctx.client.stats())),
  render(ctx) {
    return ctx.args.id ? recordScreen(ctx) : overview(ctx);
  },
};

function overview(ctx) {
  const { client, app } = ctx;
  const stats = client.stats();
  const section = h('section', { class: 'screen screen-data' }, h('h1', { text: 'Data' }));
  section.append(
    h('p', { class: 'lede', text: 'Everything Perch shows comes from the signed records below. You can export them, check a record, or load records from a file. Records that fail verification are rejected.' }),
  );
  section.append(
    h('dl', { class: 'stat-row', id: 'stats' },
      [
        ['records', 'Records', stats.records],
        ['authors', 'Authors', stats.authors],
        ['products', 'Products', stats.products],
        ['listings', 'Listings', stats.listings],
        ['reviews', 'Reviews', stats.reviews],
        ['forks', 'Keys with a conflicting history', stats.forks],
        ['invalid', 'Invalid (stored, ignored by views)', stats.invalid],
        ['redacted', 'Content removed by a node', stats.redacted],
      ].map(([key, k, v]) => h('div', { class: 'stat', dataset: { stat: key } }, h('dt', { text: k }), h('dd', { text: fmt.number(v) }))),
    ),
  );

  // Build and policy information.
  section.append(
    h('section', { class: 'panel', 'aria-labelledby': 'h-build' },
      h('h2', { id: 'h-build', text: 'What is running' }),
      definitionList([
        ['App version', h('span', { text: `${APP_VERSION}${app.mode === 'mock' ? ' (mock client)' : ''}` })],
        ['Client version', h('span', { text: app.build?.version ?? 'unknown' })],
        ['Wasm SHA-256', app.build?.wasm_sha256 ? h('code', { class: 'hex hex-full', id: 'wasm-sha', text: app.build.wasm_sha256 }) : h('span', { text: 'none (mock client)' })],
        ['Policy hash', h('code', { class: 'hex hex-full', text: client.policyHash() })],
      ]),
      h('p', { class: 'hint', text: 'Compare the wasm SHA-256 with the value published for this release. A different value means this page runs different code.' }),
    ),
  );

  // Look up a record.
  const idIn = h('input', { id: 'lookup-id', class: 'mono', spellcheck: 'false', autocomplete: 'off', placeholder: '64 hex characters' });
  const lookup = h('form', { class: 'panel', id: 'lookup' },
    h('h2', { text: 'Find a record' }),
    field({ id: 'lookup-id', label: 'Record ID', control: idIn }),
    h('button', { type: 'submit', class: 'button', text: 'Open' }),
  );
  lookup.addEventListener('submit', (ev) => {
    ev.preventDefault();
    location.hash = routes.record(idIn.value.trim().toLowerCase());
  });
  section.append(lookup);

  // Verify a record.
  const vIn = h('textarea', { id: 'verify-json', rows: 6, class: 'mono', spellcheck: 'false' });
  const vOut = h('div', { role: 'status' });
  const verify = h('form', { class: 'panel', id: 'verify' },
    h('h2', { text: 'Check a record' }),
    field({ id: 'verify-json', label: 'Record JSON', hint: 'Checks the ID, body hash, and signature without storing the record.', control: vIn }),
    h('button', { type: 'submit', class: 'button', text: 'Check' }),
    vOut,
  );
  verify.addEventListener('submit', (ev) => {
    ev.preventDefault();
    const res = client.verifyRecord(vIn.value.trim());
    vOut.replaceChildren(
      res.valid
        ? notice('good', 'Valid record', h('p', {}, 'ID ', hex(res.id, { n: 64 }), '.'))
        : notice('bad', 'Invalid record', h('p', { text: res.error || 'Unknown error.' }), res.id ? h('p', {}, 'Computed ID ', hex(res.id, { n: 64 })) : null),
    );
  });
  section.append(verify);

  // Import.
  const file = h('input', { type: 'file', id: 'import-jsonl', accept: '.jsonl,.json,application/json,text/plain' });
  const paste = h('textarea', { id: 'import-text', rows: 4, class: 'mono', spellcheck: 'false' });
  const iOut = h('div', { role: 'status', id: 'import-result' });
  const imp = h('form', { class: 'panel', id: 'import' },
    h('h2', { text: 'Load records' }),
    field({ id: 'import-jsonl', label: 'JSONL or JSON file', control: file }),
    field({ id: 'import-text', label: 'Or paste records', hint: 'One record per line, or a JSON array.', control: paste }),
    h('button', { type: 'submit', class: 'button primary', text: 'Verify and load' }),
    iOut,
  );
  imp.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    let text = paste.value.trim();
    if (file.files?.[0]) text = await file.files[0].text();
    if (!text) {
      iOut.replaceChildren(notice('bad', 'Nothing to load', h('p', { text: 'Choose a file or paste records.' })));
      return;
    }
    const res = client.ingest(text);
    announce(`${res.inserted} records loaded, ${res.rejected} rejected.`);
    iOut.replaceChildren(ingestReport(res));
  });
  section.append(imp);

  // Export.
  section.append(
    h('section', { class: 'panel' },
      h('h2', { text: 'Export' }),
      h('p', { text: 'Download every record on this device as JSON Lines. The file is the same format nodes use, so you can load it into any node or tool.' }),
      h('button', {
        type: 'button',
        class: 'button',
        id: 'export',
        on: {
          click: () => {
            downloadText(`perch-${new Date().toISOString().slice(0, 10)}.jsonl`, client.exportJsonl(), 'application/x-ndjson');
            announce('Export downloaded.');
          },
        },
      }, `Download ${fmt.plural(stats.records, 'record')}`),
    ),
  );
  return section;
}

export function ingestReport(res) {
  const ok = res.rejected === 0;
  return notice(ok ? 'good' : 'warn', ok ? 'Records loaded' : 'Some records were rejected',
    h('ul', { class: 'plain', id: 'ingest-counts' },
      h('li', { text: `${fmt.number(res.inserted)} new` }),
      h('li', { text: `${fmt.number(res.duplicate)} already present` }),
      h('li', { text: `${fmt.number(res.gap)} waiting for earlier records` }),
      h('li', { text: `${fmt.number(res.fork)} fork evidence` }),
      h('li', { dataset: { rejected: String(res.rejected) }, text: `${fmt.number(res.rejected)} rejected` }),
    ),
    res.errors?.length
      ? h('details', {}, h('summary', { text: `Errors (${res.errors.length})` }),
          h('ul', { class: 'errors' }, res.errors.slice(0, 50).map((e) => h('li', {}, e.index != null ? `Line ${e.index + 1}: ` : '', userText('span', e.error || String(e))))))
      : null,
  );
}

function recordScreen(ctx) {
  const { client, args } = ctx;
  const id = args.id;
  const rec = /^[0-9a-f]{64}$/.test(id) ? client.record(id) : null;
  const section = h('section', { class: 'screen screen-record' },
    h('p', { class: 'crumbs' }, h('a', { href: '#/data', text: 'Data' }), ' / record'),
    h('h1', { text: rec ? `Record: ${rec.kind}` : 'Record not found' }),
  );
  if (!rec) {
    section.append(notice('info', null, h('p', {}, 'No record with ID ', hex(id, { n: 64 }), ' is on this device.')));
    return section;
  }
  const check = client.verifyRecord(JSON.stringify(rec));
  section.append(
    check.valid
      ? notice('good', 'Signature and hashes verify', h('p', { text: 'This device recomputed the ID and body hash and checked the signature.' }))
      : notice('bad', 'Record does not verify', h('p', { text: check.error })),
  );
  const authorView = client.authorView(rec.author);
  section.append(
    definitionList([
      ['ID', h('code', { class: 'hex hex-full', id: 'record-id', text: id })],
      ['Author', authorChip(rec.author, authorView?.name ?? null, { verified: Boolean(authorView?.verified), status: authorView?.status })],
      ['Position in log', h('span', { text: `${rec.seq}` })],
      ['Previous record', rec.prev ? recordLink(rec.prev) : h('span', { text: 'None (first record)' })],
      ['Replaces', rec.replaces ? recordLink(rec.replaces) : null, 'No earlier version'],
      ['Kind', h('code', { text: rec.kind })],
      ['Claimed time', h('span', { text: `${fmt.dateTime(rec.ts)} (${rec.ts})` })],
      ['Body hash', h('code', { class: 'hex hex-full', text: rec.body_hash })],
      ['Signature', h('code', { class: 'hex hex-full sig', text: rec.sig })],
      ['Body', 'body' in rec
        ? h('span', { text: check.valid ? 'Present (hash matches)' : 'Present' })
        : h('span', { text: 'Removed by a node. The header and signature remain.' })],
      ['Status in views', statusInViews(client, id)],
    ]),
  );
  const related = relatedLinks(client, rec);
  if (related.length) section.append(h('p', { class: 'related' }, ...related));
  section.append(
    h('section', { class: 'block' },
      h('h2', { text: 'Signed JSON' }),
      h('pre', { class: 'json', id: 'record-json', tabindex: '0', text: JSON.stringify(rec, null, 2) }),
      h('p', {}, h('button', { type: 'button', class: 'button small', on: { click: () => downloadText(`${id}.json`, JSON.stringify(rec) + '\n') } }, 'Download record')),
    ),
  );
  section.append(verifyYourself(id));
  return section;
}

// RecordStatus as text (docs/ux.md 8.9.2 item 3).
function statusInViews(client, id) {
  let st = null;
  try {
    st = typeof client.recordStatus === 'function' ? client.recordStatus(id) : null;
  } catch {
    st = null;
  }
  if (!st) return h('span', { class: 'muted', text: 'Unknown' });
  const parts = [];
  if (!st.semantic_valid) parts.push(h('p', { class: 'warn-text', dataset: { testid: 'status-ignored' } }, 'Ignored: ', ignoredText(st.reason) || 'it breaks the format rules.'));
  else if (st.ignored) parts.push(h('p', { class: 'warn-text', dataset: { testid: 'status-ignored' } }, 'Ignored: ', ignoredText(st.ignored)));
  else parts.push(h('p', { dataset: { testid: 'status-used' }, text: 'Used by views.' }));
  const versions = st.versions || [];
  if (versions.length > 1) {
    const pos = versions.indexOf(id);
    const latest = versions[versions.length - 1];
    parts.push(h('p', {},
      `Version ${pos + 1} of ${versions.length}. `,
      st.root !== id ? ['First version: ', recordLink(st.root), '. '] : null,
      latest !== id ? h('span', { dataset: { testid: 'replaced-by' } }, 'Replaced by ', recordLink(versions[pos + 1] ?? latest), latest !== versions[pos + 1] ? ['; latest: ', recordLink(latest)] : null, '.') : 'This is the latest version.',
    ));
  }
  return h('div', { class: 'record-status' }, parts);
}

const VERIFY_COMMANDS = (file) => `# ${file} holds one record line exactly as exported.
jq -jcS 'del(.sig, .body)' ${file} > header.json   # canonical header
sha256sum header.json                                   # equals the record ID
jq -jcS '.body' ${file} | sha256sum                 # equals body_hash (see note)
{ printf 'perch/record/v1\n'; cat header.json; } > signed.bin
{ printf '302a300506032b6570032100'; jq -r .author ${file}; } | tr -d '\n' \
  | perl -pe 's/(..)/chr hex $1/ge' > author.der
jq -jr .sig ${file} | perl -pe 's/(..)/chr hex $1/ge' > sig.bin
openssl pkeyutl -verify -pubin -keyform DER -inkey author.der -rawin \
  -in signed.bin -sigfile sig.bin                       # "Signature Verified Successfully"`;

// "Verify it yourself" (docs/ux.md 8.9.2 item 6).
function verifyYourself(id) {
  const file = `${id}.json`;
  const text = VERIFY_COMMANDS(file);
  return h('details', { class: 'block verify-yourself', dataset: { testid: 'verify-yourself' } },
    h('summary', {}, h('h2', { class: 'inline', text: 'Verify it yourself' })),
    h('p', { text: `Download the record, then run these commands with jq and OpenSSL. They do not use Perch code.` }),
    h('pre', { class: 'json', tabindex: '0', role: 'region', 'aria-label': 'Verification commands', text }),
    h('p', {}, h('button', { type: 'button', class: 'button small', on: { click: () => copyText(text) } }, 'Copy commands')),
    h('ul', { class: 'hint' },
      h('li', { text: 'Tested with jq 1.7 and OpenSSL 3.0.13 on a signed test record.' }),
      h('li', { text: 'Header keys are ASCII, so jq -S key order equals RFC 8785 order. For bodies, jq can differ from RFC 8785: it escapes U+007F, and it orders keys by code point, which differs from UTF-16 order for characters above U+FFFF. Use an RFC 8785 library for such bodies.' }),
      h('li', { text: 'The app uses strict Ed25519 verification. A record the app accepts also passes OpenSSL. OpenSSL may accept rare signatures that the app rejects.' }),
    ),
  );
}

function relatedLinks(client, rec) {
  const out = [];
  const b = rec.body || {};
  if (b.product && client.product(b.product)) out.push(h('a', { class: 'button small', href: routes.product(b.product), text: 'Open product' }));
  if (rec.kind === 'listing') out.push(h('a', { class: 'button small', href: routes.listing(rec.replaces ? findRoot(client, rec) : client.verifyRecord(JSON.stringify(rec)).id), text: 'Open listing' }));
  if (b.target) out.push(h('a', { class: 'button small', href: routes.record(b.target), text: 'Open target record' }));
  if (b.subject) out.push(h('a', { class: 'button small', href: routes.author(b.subject), text: 'Open subject' }));
  out.push(h('a', { class: 'button small', href: routes.author(rec.author), text: 'Open author' }));
  return out;
}

function findRoot(client, rec) {
  let cur = rec;
  for (let i = 0; i < 1000 && cur.replaces; i++) {
    const next = client.record(cur.replaces);
    if (!next) return cur.replaces;
    cur = next;
  }
  return client.verifyRecord(JSON.stringify(cur)).id;
}
