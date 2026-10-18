// Wasm-level API test in Node. Loads web/pkg (or the directory given as the
// first argument) and checks the PerchClient surface of ARCHITECTURE 8.2.
//
//   node crates/perch-wasm/tests/node-api.mjs [pkgDir]
//
// Store- and view-dependent checks run when the backend is wired in
// (stats().records > 0 after ingesting demo data, given as PERCH_DEMO=file).

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const pkg = path.resolve(process.argv[2] || path.join(here, '../../../web/pkg'));
const mod = await import(pathToFileURL(path.join(pkg, 'perch_wasm.js')).href);
await mod.default({ module_or_path: fs.readFileSync(path.join(pkg, 'perch_wasm_bg.wasm')) });
const { PerchClient } = mod;

let failed = 0;
let passed = 0;
const check = (name, ok, detail = '') => {
  if (ok) passed++;
  else failed++;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
};
const throws = (fn) => {
  try {
    fn();
    return null;
  } catch (e) {
    return e;
  }
};

const METHODS = [
  'generateKey', 'exportKey', 'importKey', 'author', 'publish', 'publishStatus', 'issueReceipt', 'verifyReceipt',
  'ingest', 'connect', 'disconnect', 'peers', 'setPolicy', 'policy', 'policyHash', 'search', 'product', 'listing',
  'authorView', 'record', 'authorLog', 'authorRecords', 'flags', 'recordStatus', 'currencies', 'verifyRecord',
  'stats', 'buildInfo', 'exportJsonl', 'onChange',
];

const c = PerchClient.create({});
check('create() returns a client', Boolean(c));
const missing = METHODS.filter((m) => typeof c[m] !== 'function');
check('every 8.2 method exists', missing.length === 0, missing.join(', '));

check('author() is null before a key', c.author() == null);
const author = c.generateKey();
check('generateKey() returns 64 hex', /^[0-9a-f]{64}$/.test(author));
check('author() matches', c.author() === author);

const backup = JSON.parse(c.exportKey());
check('exportKey() backup shape', /^[0-9a-f]{64}$/.test(backup.secret) && backup.head === null);

const d = PerchClient.create({});
check('importKey(backup) gives the same author', d.importKey(JSON.stringify(backup)) === author);
const st = d.publishStatus();
check('imported key without nodes is not ready', st.ready === false && typeof st.reason === 'string', st.reason);
check('publishStatus() shape', 'local_head' in st && Array.isArray(st.nodes));
check('importKey(bare secret)', PerchClient.create({}).importKey(backup.secret) === author);
check('importKey rejects garbage', throws(() => PerchClient.create({}).importKey('xyz')) !== null);

const s = PerchClient.create({ sparse: true });
s.generateKey();
check('sparse client is not ready to publish', /sparse/.test(s.publishStatus().reason || ''));

let events = 0;
const unsub = c.onChange((ev) => {
  if (ev && typeof ev.reason === 'string') events++;
});
c.setPolicy({ ...c.policy(), flag_threshold: 4 });
check('onChange fires on setPolicy', events === 1);
unsub();
c.setPolicy({ ...c.policy(), flag_threshold: 3 });
check('unsubscribe stops events', events === 1);
check('policy() round-trips', c.policy().flag_threshold === 3);

const info = c.buildInfo();
check('buildInfo().version', typeof info.version === 'string');
mod.setWasmSha256('ab'.repeat(32));
check('setWasmSha256 sets buildInfo().wasm_sha256', c.buildInfo().wasm_sha256 === 'ab'.repeat(32));

const stats = c.stats();
check('stats() shape', ['records', 'authors', 'products', 'listings', 'reviews', 'forks', 'invalid', 'redacted'].every((k) => typeof stats[k] === 'number'));
check('peers() is an array', Array.isArray(c.peers()));
check('search() returns an array', Array.isArray(c.search('', {})));
check('product(unknown) is null', c.product('gtin:00000000000000') === null);
check('exportJsonl() is a string', typeof c.exportJsonl() === 'string');

const demo = process.env.PERCH_DEMO;
if (demo) {
  const res = c.ingest(fs.readFileSync(demo, 'utf8'));
  check('ingest demo data', res.inserted > 0 && res.rejected === 0, JSON.stringify({ ...res, errors: res.errors.length }));
  const policyFile = process.env.PERCH_DEMO_POLICY;
  if (policyFile) c.setPolicy(JSON.parse(fs.readFileSync(policyFile, 'utf8')));
  const hits = c.search('', { sort: 'relevance' });
  check('search over demo data', hits.length > 0);
  const p = c.product(hits[0].key);
  check('product view shape', p && p.key === hits[0].key && p.ratings && Array.isArray(p.listings));
  const line = c.exportJsonl().split('\n')[0];
  const rec = JSON.parse(line);
  check('verifyRecord(valid)', c.verifyRecord(line).valid === true);
  if (rec.body) {
    rec.body.__tamper = 1;
    check('verifyRecord(tampered) is invalid', c.verifyRecord(JSON.stringify(rec)).valid === false);
    const r2 = c.ingest([JSON.stringify(rec)]);
    check('ingest rejects a tampered record', r2.rejected === 1);
  }
  // Publish with a fresh key.
  const e = PerchClient.create({});
  e.ingest(fs.readFileSync(demo, 'utf8'));
  e.generateKey();
  const pub = e.publish('profile', { name: 'Node test', role: 'buyer' }, {});
  check('publish() returns id and record', /^[0-9a-f]{64}$/.test(pub.id) && pub.record.seq === 1);
  const pub2 = e.publish('review', { product: hits[0].key, rating: 4, text: 'Test.' }, {});
  check('second publish continues the log', pub2.record.seq === 2 && pub2.record.prev === pub.id);
  check('exportKey head follows publish', JSON.parse(e.exportKey()).head.seq === 2);
}

console.log(`\n${passed} passed, ${failed} failed.`);
process.exit(failed ? 1 : 0);
