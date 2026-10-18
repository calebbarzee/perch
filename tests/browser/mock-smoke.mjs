// Smoke test: the web app renders every screen with the mock client, with no
// console errors or page errors. Runs without perch-node or the wasm package.
//
// Usage: NODE_PATH=/opt/npm-tools/node_modules node tests/browser/mock-smoke.mjs

import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { startStaticServer } from './lib/static-server.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const here = path.dirname(fileURLToPath(import.meta.url));
const webDir = path.join(here, '../../web');
const shots = process.env.PERCH_SHOTS || path.join(process.env.TMPDIR || '/tmp', 'perch-browser-shots');
fs.mkdirSync(shots, { recursive: true });

const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
};

const { server, url } = await startStaticServer(webDir);
const browser = await chromium.launch();
const errors = [];

async function newPage(colorScheme = 'light') {
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 900 }, colorScheme });
  const page = await ctx.newPage();
  page.on('console', (m) => {
    if (m.type() === 'error' || m.type() === 'warning') errors.push(`${m.type()}: ${m.text()}`);
  });
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  return { ctx, page };
}

async function waitReady(page) {
  await page.waitForFunction(() => document.documentElement.dataset.ready === 'true', null, { timeout: 15000 });
}

async function go(page, hash) {
  await page.evaluate((h) => {
    location.hash = h;
  }, hash);
  await page.waitForTimeout(150);
}

try {
  const { ctx, page } = await newPage();
  await page.goto(`${url}/?mock=1&nodes=ws://mock-a/v1/sync,ws://mock-b/v1/sync`);
  await waitReady(page);
  await page.waitForFunction(() => document.querySelectorAll('.hit').length > 0, null, { timeout: 10000 });
  check('search shows results from synced mock data', (await page.locator('.hit').count()) >= 10);
  check('mock banner shown', await page.locator('.banner-mock').isVisible());

  await go(page, '#/search?q=kettle');
  const hitKey = await page.locator('.hit').first().getAttribute('data-key');
  check('search for kettle finds the kettle', hitKey === 'gtin:04006381333931', hitKey);

  await page.locator('.hit-name a').first().click();
  await page.waitForSelector('.screen-product');
  await page.waitForSelector('.chart-svg');
  check('product page: headline rating', (await page.locator('[data-testid=rating-verified] .big').textContent()) === '4.33');
  check('product page: listings', (await page.locator('.listing').count()) === 2);
  check('product page: price chart drawn', (await page.locator('.chart-svg .series .line').count()) >= 2);
  check('product page: reviews', (await page.locator('.review').count()) >= 8);
  await page.screenshot({ path: path.join(shots, 'mock-product.png'), fullPage: true });

  // Keyboard on chart.
  await page.locator('.chart-plot').first().focus();
  await page.keyboard.press('ArrowLeft');
  check('chart tooltip on keyboard', await page.locator('.chart-tooltip').first().isVisible());

  // Hidden listing with appeal.
  await go(page, '#/search?q=earbuds');
  await page.locator('.hit-name a').first().click();
  await page.waitForSelector('.screen-product');
  const hidden = page.locator('.listing[data-status=hidden]');
  check('hidden listing has a labeled notice', (await hidden.count()) === 1 && (await hidden.locator('[data-status=hidden] .notice-title').textContent()) === 'Hidden by moderation');
  check('appeal text shown', (await hidden.locator('.appeal blockquote').count()) === 1);
  await page.screenshot({ path: path.join(shots, 'mock-hidden.png'), fullPage: true });

  // Listing screen.
  const lid = await hidden.getAttribute('data-listing');
  await go(page, `#/listing/${lid}`);
  await page.waitForSelector('.screen-listing');
  check('listing screen renders', (await page.locator('.screen-listing h1').count()) === 1);

  // Author screen (seller).
  await page.locator('.screen-listing .author-chip').first().click();
  await page.waitForSelector('.screen-author');
  check('author screen renders with restricted badge', (await page.locator('.screen-author .badge-bad').count()) >= 1);

  // Redacted review.
  await go(page, '#/search?q=backpack');
  await page.locator('.hit-name a').first().click();
  await page.waitForSelector('.screen-product');
  check('redacted review shows notice', (await page.locator('.review[data-status=unavailable] .notice-title').textContent()) === 'Text removed');

  // Keys: create key, confirm backup.
  await go(page, '#/keys');
  await page.locator('#generate-key').click();
  await page.waitForSelector('#my-author');
  const author = (await page.locator('#my-author').textContent()).trim();
  check('key generated', /^[0-9a-f]{64}$/.test(author));
  await page.locator('#backup-ok').check();
  await page.screenshot({ path: path.join(shots, 'mock-keys.png'), fullPage: true });

  // Publish a review.
  await go(page, '#/publish?kind=review&product=gtin:04006381333931');
  await page.waitForSelector('#form-review');
  await page.locator('.rating-option').nth(3).click();
  await page.fill('#f-text', 'Test review from the smoke test.');
  await page.locator('#form-review button[type=submit]').click();
  await page.waitForSelector('#form-review .notice-good');
  check('review published', true);

  // Receipt tool (no listings for this key).
  await go(page, '#/publish?kind=receipt');
  await page.waitForSelector('#form-receipt');
  check('receipt tool renders', true);
  for (const k of ['listing', 'product', 'profile', 'flag', 'appeal', 'retract']) {
    await go(page, `#/publish?kind=${k}`);
    await page.waitForSelector(`#form-${k}`);
  }
  check('all publish forms render', true);

  await go(page, '#/network');
  await page.waitForSelector('#peers-table');
  check('network shows 2 nodes', (await page.locator('#peers-table tbody tr').count()) === 2);
  check('network shows disagreement', (await page.locator('.screen-network .notice-warn').count()) === 1);

  await go(page, '#/policy');
  await page.waitForSelector('#policy-hash');
  check('policy hash shown', /^[0-9a-f]{64}$/.test((await page.locator('#policy-hash').textContent()).trim()));

  await go(page, '#/data');
  await page.waitForSelector('#stats');
  check('data screen renders', true);

  // Tampered record rejected.
  const tampered = await page.evaluate(async () => {
    const { getClient } = await import('./src/client.js');
    const c = getClient();
    const p = c.product('gtin:04006381333931');
    const r = c.record(p.listings[0].latest);
    r.body.price.amount = 1;
    return JSON.stringify(r);
  });
  await page.fill('#import-text', tampered);
  await page.locator('#import button[type=submit]').click();
  await page.waitForSelector('#import-result [data-rejected]');
  check('tampered record rejected by ingest', (await page.locator('#import-result [data-rejected]').getAttribute('data-rejected')) === '1');

  const rid = await page.evaluate(async () => {
    const { getClient } = await import('./src/client.js');
    return getClient().product('gtin:04006381333931').listings[0].latest;
  });
  await go(page, `#/data/${rid}`);
  await page.waitForSelector('#record-json');
  check('record inspector renders', (await page.locator('.screen-record .notice-good').count()) === 1);

  // C5: hidden product records come last, collapsed.
  await go(page, '#/search?q=charger');
  await page.waitForSelector('.moderated-hits');
  check('search groups hits with hidden product records', (await page.locator('.moderated-hits .hit').count()) === 1);
  // C6: why as sentences with a raw toggle.
  await go(page, '#/search?q=kettle');
  await page.locator('.why summary').first().click();
  const why = await page.locator('.why li').first().textContent();
  check('why shown as sentences', /^Name contains "kettle": \+30$/.test(why), why);
  check('raw why toggle', (await page.locator('.why .link-button').first().textContent()) === 'Show raw');
  // C8: histogram rows set the star filter.
  await go(page, `#/product/${encodeURIComponent('gtin:04006381333931')}`);
  await page.waitForSelector('.histogram button');
  await page.locator('.histogram button').first().click();
  const shownStars = await page.locator('.review:not([hidden]) .stars').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));
  check('histogram button filters reviews by stars', shownStars.length > 0 && shownStars.every((l) => l.startsWith('5 ')), shownStars.join('; '));
  check('star filter stored in the URL', (await page.evaluate(() => location.hash)).includes('stars=5'));
  // C4: buttons are never disabled; the gate shows on press.
  await go(page, '#/publish?kind=receipt');
  check('Sign receipt is enabled', await page.locator('#form-receipt button[type=submit]').isEnabled());
  await page.locator('#form-receipt button[type=submit]').click();
  check('Sign receipt without listings explains why', (await page.locator('#form-receipt .notice').count()) === 1);

  await go(page, '#/nowhere');
  check('unknown route shows not found', (await page.locator('h1').textContent()) === 'Page not found');

  // Reload keeps key and records.
  await page.waitForTimeout(1300);
  await page.reload();
  await waitReady(page);
  await go(page, '#/keys');
  await page.waitForSelector('#my-author');
  check('reload keeps key', (await page.locator('#my-author').textContent()).trim() === author);
  await ctx.close();

  // Dark mode and narrow screen.
  const dark = await newPage('dark');
  await dark.page.setViewportSize({ width: 375, height: 800 });
  await dark.page.goto(`${url}/?mock=1&nodes=ws://mock-a/v1/sync#/product/${encodeURIComponent('gtin:04006381333931')}`);
  await waitReady(dark.page);
  await dark.page.waitForSelector('.chart-svg');
  const overflow = await dark.page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  check('no horizontal scroll at 375 px', overflow <= 0, `overflow ${overflow}px`);
  // C11: Menu disclosure below 600 px.
  check('nav collapsed behind a Menu button', (await dark.page.locator('#menu-button').isVisible()) && !(await dark.page.locator('#topnav-list').isVisible()));
  await dark.page.click('#menu-button');
  check('Menu opens the nav', (await dark.page.locator('#topnav-list').isVisible()) && (await dark.page.getAttribute('#menu-button', 'aria-expanded')) === 'true');
  await dark.page.keyboard.press('Escape');
  check('Escape closes the menu', !(await dark.page.locator('#topnav-list').isVisible()));
  await dark.page.setViewportSize({ width: 360, height: 800 });
  await dark.page.waitForTimeout(300);
  const overflow360 = await dark.page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  check('no horizontal scroll at 360 px', overflow360 <= 0, `overflow ${overflow360}px`);
  await dark.page.screenshot({ path: path.join(shots, 'mock-product-dark-narrow.png'), fullPage: true });
  await dark.ctx.close();
} catch (e) {
  check('smoke test ran to completion', false, e.message);
} finally {
  const filtered = errors.filter((e) => !/favicon/.test(e));
  check('no console errors', filtered.length === 0, filtered.slice(0, 5).join(' | '));
  await browser.close();
  server.close();
}

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed. Screenshots: ${shots}`);
process.exit(failed.length ? 1 : 0);
