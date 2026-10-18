// Browser extension test (Manifest V3, headless Chromium).
//
//   node tests/browser/extension.mjs          real mode: perch-node + wasm (extension/pkg)
//   node tests/browser/extension.mjs --mock   mock mode: fake nodes + mock client
//
// Both modes load the unpacked extension with launchPersistentContext and
// --load-extension. Chromium 141 runs extensions only in the new headless
// mode, which Playwright selects with channel: 'chromium'.
//
// Checks: the panel appears on a page with JSON-LD for a demo product; it
// shows the identity-verified rating, the verified-purchase split, the price
// range, and the listing count computed from the same records; the lookup
// request carries only the 4-hex prefix; a page with microdata works; a page
// without a GTIN gets no panel; two nodes that disagree are reported.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { createHash } from 'node:crypto';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
import { startStaticServer } from './lib/static-server.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../..');
const MOCK = process.argv.includes('--mock');
const shots = process.env.PERCH_SHOTS || path.join(os.tmpdir(), 'perch-browser-shots');
fs.mkdirSync(shots, { recursive: true });

const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
};
const sha256 = (s) => createHash('sha256').update(s).digest('hex');

function productPage({ gtin, name, microdata = false }) {
  const ld = microdata
    ? ''
    : `<script type="application/ld+json">${JSON.stringify({
        '@context': 'https://schema.org',
        '@graph': [
          { '@type': 'BreadcrumbList', itemListElement: [] },
          { '@type': 'Product', name, gtin14: gtin, offers: { '@type': 'Offer', price: '49.99', priceCurrency: 'EUR' } },
        ],
      })}</script>`;
  const md = microdata
    ? `<div itemscope itemtype="https://schema.org/Product"><h1 itemprop="name">${name}</h1><meta itemprop="gtin13" content="${gtin}"></div>`
    : `<h1>${name}</h1>`;
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>${name} – Example Shop</title>${ld}</head><body>${md}<p>Example shop page.</p></body></html>`;
}

// Mock mode: build a temporary extension whose pkg/ is the mock client.
function mockExtension(policy) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perch-ext-'));
  fs.cpSync(path.join(repo, 'extension'), dir, { recursive: true });
  fs.rmSync(path.join(dir, 'pkg'), { recursive: true, force: true });
  fs.mkdirSync(path.join(dir, 'pkg/mock'), { recursive: true });
  for (const f of fs.readdirSync(path.join(repo, 'web/src'))) {
    if (f.endsWith('.js')) fs.copyFileSync(path.join(repo, 'web/src', f), path.join(dir, 'pkg/mock', f));
  }
  fs.writeFileSync(path.join(dir, 'pkg/perch_wasm.js'), "export { PerchClient } from './mock/mock-client.js';\nexport default async function init() {}\n");
  fs.writeFileSync(path.join(dir, 'default-policy.json'), JSON.stringify(policy, null, 2));
  fs.mkdirSync(path.join(dir, 'fonts'), { recursive: true });
  return dir;
}

// Mock mode: a fake node that answers /v1/lookup/{prefix} with JSONL.
function fakeNode(records, keys, { drop = () => false } = {}) {
  const prefixes = new Set(keys.map((k) => sha256(k).slice(0, 4)));
  const log = [];
  const server = http.createServer((req, res) => {
    log.push({ url: req.url, headers: req.headers });
    const m = /^\/v1\/lookup\/([0-9a-f]{4})$/.exec(req.url);
    if (!m) {
      res.writeHead(404).end();
      return;
    }
    const body = prefixes.has(m[1]) ? records.filter((r) => !drop(r)).map((r) => JSON.stringify(r)).join('\n') + '\n' : '';
    res.writeHead(200, { 'content-type': 'application/x-ndjson', 'access-control-allow-origin': '*' });
    res.end(body);
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve({ server, url: `http://127.0.0.1:${server.address().port}`, log })));
}

// Real mode: two perch-node processes with the demo dataset; the extension
// runs the wasm client from extension/pkg with the demo policy.
async function realSetup() {
  const { PerchNode, freePort, nodeBinary, generateDemo, tempDir, waitFor } = await import('./lib/nodes.mjs');
  if (!fs.existsSync(path.join(repo, 'extension/pkg/perch_wasm_bg.wasm'))) {
    console.log('extension/pkg is missing. Run scripts/build-wasm.sh first.');
    process.exit(2);
  }
  const bin = nodeBinary();
  const work = tempDir('perch-ext-live-');
  const demo = generateDemo(bin, work);
  const [pa, pb] = [await freePort(), await freePort()];
  const a = new PerchNode({ bin, name: 'node-a', port: pa, dataDir: path.join(work, 'a'), policyFile: demo.policyFile, importFile: demo.file });
  const b = new PerchNode({ bin, name: 'node-b', port: pb, dataDir: path.join(work, 'b'), policyFile: demo.policyFile, peers: [a.ws] });
  await a.start();
  await b.start();
  const want = (await a.stats()).records;
  await waitFor(async () => (await b.stats()).records === want, { timeout: 60000, what: 'node B sync' });

  // A product with a GTIN, identity-verified ratings, and a price.
  const hits = await a.get('/v1/search?q=&limit=1000');
  const hit = hits.find((x) => x.key.startsWith('gtin:') && x.rating_count > 0 && x.min_price) || hits.find((x) => x.key.startsWith('gtin:'));
  const key = hit.key;
  const view = await a.get(`/v1/products/${encodeURIComponent(key)}`);
  const verified = view.listings.find((l) => l.domain_verified && l.domain);

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'perch-ext-'));
  fs.cpSync(path.join(repo, 'extension'), dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'default-policy.json'), JSON.stringify(demo.policy, null, 2));
  const wrap = (n) => ({ url: n.http, log: [], server: { close: () => n.stop() } });
  const other = hits.find((x) => x.key.startsWith('gtin:') && x.key !== key);
  return {
    extDir: dir,
    nodeA: wrap(a),
    nodeB: wrap(b),
    key,
    view,
    expectDisagree: false,
    verifiedHost: verified ? { host: `shop.${verified.domain.domain}`, seller: verified.seller_name } : null,
    otherGtin: other ? other.key.slice(5) : '05012345678900',
    clientLog: true,
  };
}

async function mockSetup() {
  const { buildMockDataset } = await import(pathToFileURL(path.join(repo, 'web/src/mock-data.js')).href);
  const { PerchClient } = await import(pathToFileURL(path.join(repo, 'web/src/mock-client.js')).href);
  const ds = buildMockDataset();
  const policy = { ...ds.policy, now: null };
  const key = 'gtin:04006381333931';

  // Expected numbers from the same records and policy.
  const ref = await PerchClient.create();
  ref.setPolicy(policy);
  ref.ingest(ds.records);
  const view = ref.product(key);

  const extDir = mockExtension(policy);
  const keys = Object.values(ds.keys).filter(Boolean);
  const nodeA = await fakeNode(ds.records, keys);
  // Node B withholds the reviews by two authors.
  const hiddenAuthors = new Set([ds.authors.ana, ds.authors.farid]);
  const nodeB = await fakeNode(ds.records, keys, { drop: (r) => r.kind === 'review' && hiddenAuthors.has(r.author) });
  return {
    extDir,
    nodeA,
    nodeB,
    key,
    view,
    expectDisagree: true,
    verifiedHost: { host: 'www.brightline.example', seller: 'Brightline Electronics' },
    otherGtin: '05012345678900',
    clientLog: false,
  };
}

async function main() {
  const setup = MOCK ? await mockSetup() : await realSetup();
  const { extDir, nodeA, nodeB, key, view } = setup;
  console.log(`mode: ${MOCK ? 'mock' : 'real perch-node + wasm'}; product ${key}`);
  const gtin = key.slice(5);
  const vi = view.ratings.verified_identity;
  const vp = view.ratings.verified_purchase;
  const ranges = Object.entries(view.price_range).filter(([, r]) => r.count > 0).sort((x, y) => y[1].count - x[1].count || (x[0] < y[0] ? -1 : 1));
  const visibleListings = view.listings.filter((l) => l.status === 'visible').length;
  const fmtMean = (m) => new Intl.NumberFormat(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 2 }).format(m);

  const shop = await startStaticServer(fs.mkdtempSync(path.join(os.tmpdir(), 'perch-shop-')));
  const shopRoot = shop.server.address && path.join(os.tmpdir());
  void shopRoot;
  const pages = {
    '/jsonld.html': productPage({ gtin, name: 'Kestrel K2 electric kettle' }),
    '/microdata.html': productPage({ gtin, name: 'Kestrel K2 electric kettle', microdata: true }),
    '/none.html': '<!doctype html><title>No product</title><p>Nothing here.</p>',
  };
  shop.server.removeAllListeners('request');
  shop.server.on('request', (req, res) => {
    const body = pages[new URL(req.url, 'http://x').pathname];
    if (!body) return res.writeHead(404).end();
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(body);
  });
  // Serve the shop on "localhost" so it is a different origin from the nodes.
  const shopUrl = shop.url.replace('127.0.0.1', 'localhost');

  pages['/multi.html'] = `<!doctype html><title>Kettles</title><script type="application/ld+json">${JSON.stringify([
    { '@context': 'https://schema.org', '@type': 'Product', name: 'A', gtin14: gtin },
    { '@context': 'https://schema.org', '@type': 'Product', name: 'B', gtin14: setup.otherGtin },
  ])}</script><p>Two products.</p>`;

  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'perch-ext-profile-'));
  const ctx = await chromium.launchPersistentContext(profile, {
    channel: 'chromium',
    headless: true,
    args: [`--disable-extensions-except=${extDir}`, `--load-extension=${extDir}`],
  });
  const errors = [];
  if (setup.clientLog) {
    // Real nodes: record the requests the browser sends them.
    ctx.on('request', (r) => {
      for (const n of [nodeA, nodeB]) {
        if (r.url().startsWith(n.url)) n.log.push({ url: new URL(r.url()).pathname, headers: r.headers() });
      }
    });
  }
  const frameOf = async (page) => (await page.waitForSelector('iframe[title^="Perch"]', { timeout: 10000 })).contentFrame();
  const noPanel = async (page, url) => {
    await page.goto(url);
    await page.waitForTimeout(2500);
    return (await page.locator('iframe[title^="Perch"]').count()) === 0;
  };
  try {
    let sw = ctx.serviceWorkers()[0];
    if (!sw) sw = await ctx.waitForEvent('serviceworker', { timeout: 10000 });
    const id = new URL(sw.url()).host;
    check('background worker runs', /^[a-p]{32}$/.test(id), id);
    // The settings page opens on install (first run). Wait for it, so it
    // does not take over the tab the test uses next.
    let firstRun = null;
    for (let i = 0; i < 50 && !firstRun; i++) {
      firstRun = ctx.pages().find((pg) => pg.url().includes('/options.html'));
      if (!firstRun) await new Promise((r) => setTimeout(r, 100));
    }
    check('settings page opens on install', Boolean(firstRun), ctx.pages().map((pg) => pg.url()).join(' '));

    const page = await ctx.newPage();
    page.on('pageerror', (e) => errors.push(e.message));

    check('no panel before setup (toolbar button only is the default)', await noPanel(page, `${shopUrl}/jsonld.html`));

    // First-run settings: two nodes, show on the page.
    const opt = await ctx.newPage();
    opt.on('pageerror', (e) => errors.push(`options: ${e.message}`));
    await opt.goto(`chrome-extension://${id}/options.html`);
    check('options default to toolbar button only', await opt.isChecked('#show-toolbar'));
    await opt.fill('#nodes', `${nodeA.url}\n${nodeB.url}`);
    await opt.check('#show-page');
    await opt.click('button[type=submit]');
    await opt.waitForFunction(() => document.getElementById('status').textContent === 'Saved.');
    check('options page saves nodes and display mode', true);

    nodeA.log.length = 0;
    nodeB.log.length = 0;
    await page.goto(`${shopUrl}/jsonld.html`);
    const frameEl = await page.waitForSelector('iframe[title^="Perch"]', { timeout: 10000 });
    const frame = await frameEl.contentFrame();
    await frame.waitForFunction(() => {
      const t = document.querySelector('[data-testid=tab]');
      return t && !t.textContent.includes('…');
    }, null, { timeout: 15000 });
    const box = await frameEl.boundingBox();
    check('panel starts collapsed as a small tab', box.height <= 40 && box.width <= 200, `${Math.round(box.width)} x ${Math.round(box.height)}`);
    const tabText = (await frame.locator('[data-testid=tab]').textContent()).trim();
    const expectedTab = vi.count ? `Perch ${fmtMean(vi.mean)} (${vi.count})` : 'Perch: no rating';
    check('tab shows the identity-verified rating and count', tabText === expectedTab, `${tabText} vs ${expectedTab}`);

    // Keyboard activation of the tab button. (Pointer clicks into an
    // out-of-process extension iframe are intermittently dropped by headless
    // Chromium.)
    await frame.locator('[data-testid=tab]').press('Enter');
    await frame.waitForSelector('[data-testid=rating]', { timeout: 10000 });
    await page.waitForTimeout(200);
    const box2 = await frameEl.boundingBox();
    check('activating the tab expands the panel', box2.height > 100, `${Math.round(box2.height)} px`);
    if (process.env.PERCH_DEBUG) console.log('panel text:', await frame.evaluate(() => document.body.innerText));
    const text = (sel) => frame.locator(sel).first().textContent();

    if (vi.count) {
      const rating = await text('[data-testid=rating] .big');
      check('rating: identity-verified mean', rating === fmtMean(vi.mean), `${rating} vs ${vi.mean}`);
      const ratingSub = await text('[data-testid=rating] .sub');
      check('rating: identity-verified count', ratingSub.includes(`${vi.count} identity-verified`), ratingSub);
    }
    const purchase = (await text('[data-testid=purchase] dd')).trim();
    const expectedPurchase = vp.count ? `${fmtMean(vp.mean)} from ${vp.count}` : 'none';
    check('verified-purchase split', purchase === expectedPurchase, `${purchase} vs ${expectedPurchase}`);
    if (ranges.length) {
      const [cur, r] = ranges[0];
      const digits = { JPY: 0, KRW: 0, BHD: 3, KWD: 3 }[cur] ?? 2;
      const fmt = (n) => new Intl.NumberFormat(undefined, { style: 'currency', currency: cur, minimumFractionDigits: digits, maximumFractionDigits: digits }).format(n / 10 ** digits);
      const price = await text(`[data-testid=price][data-currency=${cur}] strong`);
      const expectedPrice = r.min === r.max ? fmt(r.min) : `${fmt(r.min)} to ${fmt(r.max)}`;
      check('price range', price === expectedPrice, `${price} vs ${expectedPrice}`);
    }
    const listings = await text('[data-testid=listings]');
    check('listing count', listings.startsWith(`${visibleListings} listing`), listings);
    const open = await frame.locator('[data-testid=open]').getAttribute('href');
    check('link to the product in the web app', open.endsWith(`#/product/${encodeURIComponent(key)}`), open);
    const sources = await text('.sources');
    check('sources name both nodes', sources.includes(new URL(nodeA.url).host) && sources.includes(new URL(nodeB.url).host));
    const disagreeShown = (await frame.locator('[data-testid=disagree]').count()) === 1;
    check(setup.expectDisagree ? 'disagreement between nodes shown' : 'nodes agree: no disagreement shown', disagreeShown === setup.expectDisagree);
    const prefix = sha256(key).slice(0, 4);
    const privacy = await text('[data-testid=privacy]');
    check('privacy line shows the prefix actually sent', privacy.includes(prefix) && privacy.includes('2 nodes'), privacy);
    const site = await text('[data-testid=site]');
    check('"This site" says the shop domain is not verified', site.includes('No seller in Perch has a verified domain for localhost'), site);

    await frame.locator('body').press('Escape');
    await page.waitForTimeout(200);
    const box3 = await frameEl.boundingBox();
    check('Escape collapses the panel', box3.height <= 40 && (await frame.locator('[data-testid=tab]').count()) === 1, `${Math.round(box3.height)} px`);

    const reqs = [...nodeA.log, ...nodeB.log];
    check('each node got exactly one request', nodeA.log.length === 1 && nodeB.log.length === 1, reqs.map((r) => r.url).join(' '));
    check('request carries only the 4-hex prefix', reqs.every((r) => r.url === `/v1/lookup/${prefix}`));
    check('request has no Referer and no cookies', reqs.every((r) => !r.headers.referer && !r.headers.cookie));
    check('request does not contain the GTIN or page URL', reqs.every((r) => !JSON.stringify(r).includes(gtin) && !JSON.stringify(r).includes('jsonld.html')));
    await page.screenshot({ path: path.join(shots, 'extension-tab.png') });

    await page.goto(`${shopUrl}/microdata.html`);
    const f2 = await frameOf(page);
    await f2.waitForFunction(() => !document.querySelector('[data-testid=tab]')?.textContent.includes('…'), null, { timeout: 15000 });
    check('microdata page shows the same rating', (await f2.locator('[data-testid=tab]').textContent()).trim() === tabText);

    check('no panel on a page with 2 different GTINs', await noPanel(page, `${shopUrl}/multi.html`));
    check('no panel on a page without a GTIN', await noPanel(page, `${shopUrl}/none.html`));

    // Popup content, opened as a page with the GTIN and a verified shop host.
    const pop = await ctx.newPage();
    pop.on('pageerror', (e) => errors.push(`popup: ${e.message}`));
    if (setup.verifiedHost) {
      const vh = setup.verifiedHost;
      await pop.goto(`chrome-extension://${id}/panel.html#mode=page&gtin=${gtin}&host=${vh.host}`);
      await pop.waitForSelector('[data-testid=site]', { timeout: 15000 });
      const site2 = await pop.locator('[data-testid=site]').textContent();
      check('"This site" names the verified seller of the shop domain', site2.includes(`${vh.host} is verified as the shop of ${vh.seller}`), site2);
    }
    await pop.screenshot({ path: path.join(shots, 'extension-popup.png'), fullPage: true });

    // Back to toolbar-only: the page is left unchanged.
    await opt.check('#show-toolbar');
    await opt.click('button[type=submit]');
    await opt.waitForFunction(() => document.getElementById('status').textContent === 'Saved.');
    check('toolbar button only: no panel on the page', await noPanel(page, `${shopUrl}/jsonld.html`));
  } catch (e) {
    check('extension test ran to completion', false, e.message.split('\n')[0]);
  } finally {
    check('no page errors', errors.length === 0, errors.join(' | '));
    await ctx.close();
    nodeA.server.close();
    nodeB.server.close();
    shop.server.close();
  }
}

await main();
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed.`);
process.exit(failed.length ? 1 : 0);
