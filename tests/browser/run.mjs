// Browser tests against live perch-node processes (ARCHITECTURE 11, layer 4).
//
//   NODE_PATH=/opt/npm-tools/node_modules node tests/browser/run.mjs
//
// Needs: perch-node (built if missing), web/pkg (scripts/build-wasm.sh).
// Starts two nodes on free ports, seeds node A with the demo dataset, and
// lets B sync from A. Then checks:
//   a  the app loads the wasm client, connects, and shows synced demo data
//   b  a product page shows ratings, listings, and the price chart
//   c  a review published in the browser reaches node B over HTTP and a
//      second browser connected to node B
//   d  ProductView JSON from the browser equals GET /v1/products/{key}?now=
//      for several products, under the same policy
//   e  browser ingest rejects a tampered record
//   f  a hidden listing shows a labeled moderation notice
//   g  a reload keeps the key and the records
//   h  wasm SHA-256 and policy hash are shown and correct
// Screenshots on failure go to $PERCH_SHOTS (default: OS temp dir).
// Exits non-zero on any failure.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { REPO, PerchNode, freePort, nodeBinary, generateDemo, tempDir, waitFor, sleep } from './lib/nodes.mjs';
import { startStaticServer } from './lib/static-server.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const SHOTS = process.env.PERCH_SHOTS || path.join(os.tmpdir(), 'perch-browser-shots');
fs.mkdirSync(SHOTS, { recursive: true });
const ONLY = (process.env.PERCH_ONLY || '').split(',').filter(Boolean);

const results = [];
function record(id, name, ok, detail = '') {
  results.push({ id, name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${id} ${name}${detail ? `  (${detail})` : ''}`);
}

// Canonical JSON (RFC 8785 subset) for comparing view results.
function canon(v) {
  if (v === null || typeof v !== 'object') return JSON.stringify(v);
  if (Array.isArray(v)) return `[${v.map(canon).join(',')}]`;
  return `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canon(v[k])}`).join(',')}}`;
}

function firstDiff(a, b, p = '$') {
  if (canon(a) === canon(b)) return null;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null || Array.isArray(a) !== Array.isArray(b)) {
    return `${p}: ${JSON.stringify(a)?.slice(0, 120)} != ${JSON.stringify(b)?.slice(0, 120)}`;
  }
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const k of keys) {
    const d = firstDiff(a[k], b[k], Array.isArray(a) ? `${p}[${k}]` : `${p}.${k}`);
    if (d) return d;
  }
  return `${p}: differs`;
}

async function test(id, name, fn, page) {
  if (ONLY.length && !ONLY.includes(id)) return;
  try {
    const detail = await fn();
    record(id, name, true, typeof detail === 'string' ? detail : '');
  } catch (e) {
    record(id, name, false, e.message.split('\n')[0]);
    const pages = Array.isArray(page) ? page : [page];
    for (const [i, p] of pages.entries()) {
      if (!p) continue;
      const file = path.join(SHOTS, `fail-${id}${pages.length > 1 ? `-${i}` : ''}.png`);
      await p.screenshot({ path: file, fullPage: true }).catch(() => {});
      console.log(`      screenshot: ${file}`);
    }
  }
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

async function waitReady(page) {
  await page.waitForFunction(() => document.documentElement.dataset.ready === 'true', null, { timeout: 30000 });
}

async function go(page, hash) {
  await page.evaluate((h) => {
    location.hash = h;
  }, hash);
  await page.waitForFunction((h) => location.hash === h, hash);
  await page.waitForTimeout(150);
}

async function clientEval(page, fn, arg) {
  return page.evaluate(
    async ([src, a]) => {
      const { getClient } = await import('./src/client.js');
      // eslint-disable-next-line no-new-func
      return new Function('client', 'arg', `return (${src})(client, arg);`)(getClient(), a);
    },
    [fn.toString(), arg],
  );
}

async function main() {
  // Preconditions.
  const pkg = path.join(REPO, 'web/pkg/perch_wasm_bg.wasm');
  if (!fs.existsSync(pkg)) {
    console.log('web/pkg is missing. Run scripts/build-wasm.sh first.');
    process.exit(2);
  }
  const bin = nodeBinary();
  const work = tempDir();
  console.log(`work dir: ${work}`);
  const demo = generateDemo(bin, work);
  const policyFile = path.join(REPO, 'web/default-policy.json');
  const policy = demo.policy ?? JSON.parse(fs.readFileSync(policyFile, 'utf8'));
  const attestors = (policy.trusted_attestors || []).map((a) => a.author);

  const [portA, portB] = [await freePort(), await freePort()];
  const nodeA = new PerchNode({ bin, name: 'node-a', port: portA, dataDir: path.join(work, 'a'), attestors, extraArgs: ['--web', path.join(REPO, 'web')] });
  const nodeB = new PerchNode({ bin, name: 'node-b', port: portB, dataDir: path.join(work, 'b'), attestors, peers: [`ws://127.0.0.1:${portA}/v1/sync`], extraArgs: ['--web', path.join(REPO, 'web')] });
  await nodeA.start();
  await nodeA.submit(demo.jsonl);
  await nodeB.start();
  const statsA = await nodeA.stats();
  await waitFor(async () => (await nodeB.stats()).records === statsA.records, { timeout: 60000, what: 'node B to sync from node A' });
  console.log(`nodes ready: ${statsA.records} records`);

  // The app is served by a static server so the test also covers "any host".
  // Node A also serves it (--web) for the node-served path.
  const web = await startStaticServer(path.join(REPO, 'web'));
  const appA = `${web.url}/?nodes=${encodeURIComponent(nodeA.ws)}`;
  const appB = `${web.url}/?nodes=${encodeURIComponent(nodeB.ws)}`;

  const browser = await chromium.launch();
  const errors = [];
  const watch = (page, label) => {
    page.on('pageerror', (e) => errors.push(`${label} pageerror: ${e.message}`));
    page.on('console', (m) => {
      if (m.type() === 'error' && !/WebSocket connection/.test(m.text())) errors.push(`${label} console: ${m.text()}`);
    });
  };

  // Context 1: connected to node A. A persistent profile dir is not needed:
  // IndexedDB survives reloads within one context.
  const ctx1 = await browser.newContext({ viewport: { width: 1200, height: 900 } });
  const page = await ctx1.newPage();
  watch(page, 'ctx1');
  await page.goto(appA);
  await waitReady(page);

  // Apply the demo policy, if the demo generator provided one that differs
  // from the bundled file, through the client (as a user would on the Policy screen).
  await clientEval(page, (client, p) => {
    if (JSON.stringify(client.policy().trusted_attestors) !== JSON.stringify(p.trusted_attestors)) client.setPolicy({ ...p, now: null });
  }, policy);

  let hits = [];
  await test('a', 'app loads the wasm client, connects, and shows synced demo data', async () => {
    const mode = await page.evaluate(() => document.documentElement.dataset.client);
    assert(mode === 'wasm', `client mode is ${mode}`);
    await waitFor(async () => (await clientEval(page, (c) => c.stats().records)) === statsA.records, { timeout: 60000, what: 'browser sync' });
    const peers = await clientEval(page, (c) => c.peers());
    assert(peers.length === 1 && peers[0].state === 'live', `peers: ${JSON.stringify(peers)}`);
    await go(page, '#/search');
    await page.waitForSelector('.hit');
    hits = await clientEval(page, (c) => c.search('', {}));
    const shown = await page.locator('.hit').count();
    assert(shown === hits.length && shown > 0, `${shown} results shown, ${hits.length} expected`);
    return `${statsA.records} records, ${shown} products`;
  }, page);

  // Pick test products from the native API.
  const allHits = await nodeA.get('/v1/search?q=');
  const keys = allHits.map((h) => h.key);

  await test('b', 'product page shows ratings, listings, and the price chart', async () => {
    let chosen = null;
    for (const k of keys) {
      const v = await nodeA.get(`/v1/products/${encodeURIComponent(k)}`);
      if (v.ratings.verified_identity.count > 0 && v.listings.length >= 1 && v.listings.some((l) => l.price_history.length > 1)) {
        chosen = v;
        break;
      }
    }
    assert(chosen, 'no demo product with ratings and price history');
    await go(page, `#/product/${encodeURIComponent(chosen.key)}`);
    await page.waitForSelector('.screen-product');
    await page.waitForSelector('.chart-svg .series .line', { timeout: 10000 });
    const big = await page.locator('[data-testid=rating-verified] .big').textContent();
    const local = await clientEval(page, (c, k) => c.product(k), chosen.key);
    assert(Number(big) === local.ratings.verified_identity.mean, `headline ${big} vs ${local.ratings.verified_identity.mean}`);
    const listings = await page.locator('.screen-product .listing').count();
    assert(listings === local.listings.length, `${listings} listings shown, ${local.listings.length} in view`);
    const series = await page.locator('.chart-svg .series').count();
    assert(series >= 1, 'no chart series');
    return `${chosen.key}: ${big}, ${listings} listings, ${series} series`;
  }, page);

  await test('d', 'browser ProductView equals native GET /v1/products/{key}?now=', async () => {
    const now = Date.now() - 60_000;
    const sample = keys.slice(0, 8);
    assert(sample.length >= 3, 'fewer than 3 products');
    const browserHash = await clientEval(page, (c) => c.policyHash());
    const diffs = [];
    for (const k of sample) {
      const { body: native, headers } = await nodeA.getWithHeaders(`/v1/products/${encodeURIComponent(k)}?now=${now}`);
      const nodeHash = headers.get('x-perch-policy-hash');
      const local = await clientEval(page, (c, a) => {
        const old = c.policy();
        c.setPolicy({ ...old, now: a.now });
        const v = c.product(a.key);
        c.setPolicy(old);
        return v;
      }, { key: k, now });
      if (nodeHash && nodeHash !== browserHash) diffs.push(`${k}: policy hash ${nodeHash} (node) vs ${browserHash} (browser)`);
      const d = firstDiff(local, native);
      if (d) diffs.push(`${k}: ${d}`);
    }
    assert(!diffs.length, diffs.slice(0, 3).join(' | '));
    return `${sample.length} products identical`;
  }, page);

  await test('e', 'browser ingest rejects a tampered record', async () => {
    const res = await clientEval(page, (c) => {
      const line = c.exportJsonl().split('\n').find((l) => l.includes('"kind":"review"'));
      const r = JSON.parse(line);
      const id = c.verifyRecord(line).id;
      r.body.rating = r.body.rating === 5 ? 1 : 5;
      const t1 = c.ingest([JSON.stringify(r)]);
      const r2 = JSON.parse(line);
      r2.sig = (r2.sig[0] === 'a' ? 'b' : 'a') + r2.sig.slice(1);
      const t2 = c.ingest(JSON.stringify(r2));
      return { t1, t2, still: c.record(id)?.body?.rating === JSON.parse(line).body.rating };
    });
    assert(res.t1.rejected === 1 && res.t1.inserted === 0, `body tamper: ${JSON.stringify(res.t1)}`);
    assert(res.t2.rejected === 1 && res.t2.inserted === 0, `signature tamper: ${JSON.stringify(res.t2)}`);
    assert(res.still, 'original record changed');
    // Through the UI as well.
    await go(page, '#/data');
    const tampered = await clientEval(page, (c) => {
      const r = JSON.parse(c.exportJsonl().split('\n')[0]);
      r.ts += 1;
      return JSON.stringify(r);
    });
    await page.fill('#import-text', tampered);
    await page.click('#import button[type=submit]');
    await page.waitForSelector('#import-result [data-rejected]');
    const n = await page.locator('#import-result [data-rejected]').getAttribute('data-rejected');
    assert(n === '1', `UI reports ${n} rejected`);
  }, page);

  await test('f', 'hidden listing shows a labeled moderation notice', async () => {
    let target = null;
    for (const k of keys) {
      const v = await nodeA.get(`/v1/products/${encodeURIComponent(k)}`);
      const l = v.listings.find((x) => x.status === 'hidden');
      if (l) {
        target = { key: k, id: l.id };
        break;
      }
    }
    assert(target, 'the demo data has no hidden listing');
    await go(page, `#/product/${encodeURIComponent(target.key)}`);
    const card = page.locator(`.listing[data-listing="${target.id}"]`);
    await card.waitFor();
    assert((await card.getAttribute('data-status')) === 'hidden', 'listing not marked hidden');
    const title = await card.locator('[data-status=hidden] .notice-title').textContent();
    assert(title === 'Hidden by moderation', `notice title: ${title}`);
    const text = await card.locator('[data-status=hidden]').innerText();
    assert(/flagged this listing/.test(text) && /Reasons:/.test(text), 'notice lacks the flag count or reasons');
  }, page);

  let myAuthor = null;
  let reviewId = null;
  let reviewKey = null;
  const ctx2 = await browser.newContext({ viewport: { width: 1100, height: 900 } });
  const page2 = await ctx2.newPage();
  watch(page2, 'ctx2');

  await test('c', 'a review published in the browser reaches node B and a browser on node B', async () => {
    await go(page, '#/keys');
    await page.click('#generate-key');
    await page.waitForSelector('#my-author');
    myAuthor = (await page.locator('#my-author').textContent()).trim();
    await page.check('#backup-ok');
    reviewKey = keys[0];
    await go(page, `#/publish?kind=review&product=${encodeURIComponent(reviewKey)}`);
    await page.waitForSelector('#form-review button[type=submit]:not([disabled])', { timeout: 10000 });
    await page.locator('.rating-option').nth(3).click();
    const text = `Browser test review ${Date.now()}.`;
    await page.fill('#f-text', text);
    await page.click('#form-review button[type=submit]');
    await page.waitForSelector('#form-review .notice-good', { timeout: 10000 });
    reviewId = await page.locator('#form-review .notice-good .record-link').first().evaluate((a) => a.getAttribute('href').split('/').pop());
    assert(/^[0-9a-f]{64}$/.test(reviewId), `record id ${reviewId}`);

    const onB = await waitFor(async () => {
      const r = await fetch(`${nodeB.http}/v1/records/${reviewId}`);
      return r.ok ? r.json() : null;
    }, { timeout: 30000, what: 'review on node B' });
    assert(onB.body.text === text, 'node B has different text');

    await page2.goto(appB);
    await waitReady(page2);
    await clientEval(page2, (client, p) => client.setPolicy({ ...p, now: null }), policy);
    await waitFor(() => clientEval(page2, (c, id) => Boolean(c.record(id)), reviewId), { timeout: 60000, what: 'review in browser 2' });
    await go(page2, `#/product/${encodeURIComponent(reviewKey)}`);
    await page2.waitForSelector(`.review[data-review="${reviewId}"]`, { timeout: 10000 });
    const shown = await page2.locator(`.review[data-review="${reviewId}"] .review-text`).textContent();
    assert(shown === text, `browser 2 shows "${shown}"`);
    return `record ${reviewId.slice(0, 12)}`;
  }, [page, page2]);

  await test('g', 'a reload keeps the key and the records', async () => {
    assert(myAuthor, 'no key from test c');
    const before = await clientEval(page, (c) => c.stats().records);
    await page.waitForTimeout(1500); // persistence is debounced by 1 s
    await page.reload();
    await waitReady(page);
    const state = await clientEval(page, (c, id) => ({ author: c.author(), records: c.stats().records, has: Boolean(c.record(id)) }), reviewId);
    assert(state.author === myAuthor, `author after reload: ${state.author}`);
    assert(state.records >= before, `records after reload: ${state.records} < ${before}`);
    assert(state.has, 'own review missing after reload');
    // Restored keys wait for sync before publishing (the guard).
    await waitFor(() => clientEval(page, (c) => c.publishStatus().ready), { timeout: 30000, what: 'publish guard after reload' });
    return `${state.records} records`;
  }, page);

  await test('h', 'wasm SHA-256 and policy hash are shown and correct', async () => {
    const expected = createHash('sha256').update(fs.readFileSync(pkg)).digest('hex');
    await go(page, '#/data');
    const shown = (await page.locator('#wasm-sha').textContent()).trim();
    assert(shown === expected, `shown ${shown}, file ${expected}`);
    const hash = await clientEval(page, (c) => c.policyHash());
    const footer = await page.locator('#footer').innerText();
    assert(footer.includes(hash.slice(0, 16)), 'footer lacks the policy hash');
  }, page);

  await test('i', 'node-served app loads from node A', async () => {
    const p3 = await ctx2.newPage();
    watch(p3, 'node-served');
    await p3.goto(`${nodeA.http}/`);
    await waitReady(p3);
    const mode = await p3.evaluate(() => document.documentElement.dataset.client);
    await p3.close();
    assert(mode === 'wasm', `client mode ${mode}`);
  }, page);

  record('-', 'no page errors', errors.length === 0, errors.slice(0, 4).join(' | '));

  await browser.close();
  web.server.close();
  await nodeB.stop();
  await nodeA.stop();
  if (results.every((r) => r.ok) && !process.env.PERCH_KEEP) fs.rmSync(work, { recursive: true, force: true });
}

try {
  await main();
} catch (e) {
  record('!', 'runner', false, e.stack || e.message);
}
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed.`);
process.exit(failed.length ? 1 : 0);
