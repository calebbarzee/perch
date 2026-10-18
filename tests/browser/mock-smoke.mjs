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

// Saved nodes are set through settings, as a user would save them; the
// ?nodes= parameter only adds nodes for one session (ARCHITECTURE 10).
async function newPage(colorScheme = 'light', { nodes = ['ws://mock-a/v1/sync', 'ws://mock-b/v1/sync'], claimTimeoutMs = null } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 900 }, colorScheme });
  await ctx.addInitScript(({ nodes, claimTimeoutMs }) => {
    if (claimTimeoutMs != null) globalThis.PERCH_MOCK_CLAIM_TIMEOUT_MS = claimTimeoutMs;
    try {
      if (!localStorage.getItem('perch.settings.v1')) localStorage.setItem('perch.settings.v1', JSON.stringify({ nodes }));
    } catch {
      // about:blank has no storage
    }
  }, { nodes, claimTimeoutMs });
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
  await page.goto(`${url}/?mock=1`);
  await waitReady(page);
  check('empty query asks for a query instead of searching', (await page.locator('.hit').count()) === 0 && (await page.locator('.empty-title').count()) === 1);
  await go(page, '#/search?q=kitchen');
  await page.waitForFunction(() => document.querySelectorAll('.hit').length > 0, null, { timeout: 10000 });
  check('search shows results from synced mock data', (await page.locator('.hit').count()) >= 3);
  check('mock banner shown', await page.locator('.banner-mock').isVisible());

  await go(page, '#/search?q=kettle');
  const hitKey = await page.locator('.hit').first().getAttribute('data-key');
  check('search for kettle finds the kettle', hitKey === 'gtin:04006381333931', hitKey);

  await page.locator('.hit-name a').first().click();
  await page.waitForSelector('.screen-product');
  await page.waitForSelector('.chart-svg');
  check('product page: headline rating', (await page.locator('[data-testid=rating-verified] .big').textContent()) === '4.33');
  check('product page: listings', (await page.locator('.listing').count()) === 3);
  check('product page: price chart drawn', (await page.locator('.chart-svg .series .line').count()) >= 1);
  // B1: listings in the price summary come first; the cheaper listing on an
  // unverified shop domain is grouped below and left out of the default chart.
  {
    const first = page.locator('.listing').first();
    const firstGroup = await first.evaluate((li) => li.closest('[data-group]')?.dataset.group);
    const firstSeller = await first.locator('.author-name').first().textContent();
    const unverified = page.locator('[data-group=domain_unverified] .listing');
    const groupTitle = await page.locator('[data-group=domain_unverified] summary').textContent();
    check('B1: price-summary listings lead the list', firstGroup === 'feeds' && firstSeller === 'Brightline Electronics', `${firstGroup} ${firstSeller}`);
    check('B1: unverified-domain listings grouped under their own heading', (await unverified.count()) === 2 && groupTitle.startsWith('Not in the price summary: shop domain not verified'), groupTitle);
    check('B1: heading says Listings, not Where to buy', (await page.locator('#h-listings').textContent()).startsWith('Listings ('));
    const chosen = await page.locator('.chart-chooser input:checked').count();
    const chosenLabel = await page.locator('.chart-chooser li:has(input:checked) label').first().textContent();
    check('B1: default chart shows only price-summary listings', chosen === 1 && chosenLabel.startsWith('Brightline'), `${chosen} ${chosenLabel}`);
  }
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
  // M1: the notice comes first; title, price, and shop link are collapsed.
  {
    const firstIsNotice = await hidden.evaluate((li) => li.firstElementChild?.dataset.status === 'hidden');
    const body = hidden.locator('.listing-hidden-body');
    check('M1: hidden listing starts with the moderation notice', firstIsNotice);
    check('M1: title, price, and shop link collapsed', !(await body.isVisible()) && !(await hidden.locator('a.external').first().isVisible()));
    await hidden.getByRole('button', { name: 'Show listing' }).click();
    check('M1: Show listing reveals them', (await body.isVisible()) && (await body.textContent()).includes('ORIGINAL'));
  }
  await page.screenshot({ path: path.join(shots, 'mock-hidden.png'), fullPage: true });

  // Listing screen.
  const lid = await hidden.getAttribute('data-listing');
  await go(page, `#/listing/${lid}`);
  await page.waitForSelector('.screen-listing');
  check('listing screen renders', (await page.locator('.screen-listing h1').count()) === 1);

  // Author screen (seller).
  await page.locator('.screen-listing .author-chip .author-link').first().click();
  await page.waitForSelector('.screen-author');
  check('author screen renders with restricted badge', (await page.locator('.screen-author .badge-bad').count()) >= 1);

  // M5: flags and appeal on a visible listing.
  await go(page, `#/product/${encodeURIComponent('gtin:08712345000172')}`);
  await page.waitForSelector('.screen-product');
  {
    const cedar = page.locator('.listing[data-status=visible]:has(.flag-summary)');
    const summary = cedar.locator('[data-testid=flag-summary]');
    check('M5: visible flagged listing shows the flag count', (await cedar.count()) === 1 && (await summary.textContent()).includes('1 flag'));
    check('M5: the appeal note is shown', ((await summary.locator('[data-testid=appeal] blockquote').textContent()) || '').includes('box design changed'));
    await summary.locator('.flag-list summary').click();
    await page.waitForSelector('.flag-summary .flags li');
    check('M5: Show flags lists the flag', ((await summary.locator('.flags li').first().textContent()) || '').includes('The photo shows a different model.'));
  }

  // M6: a badge shows its sentence on press.
  {
    const b = page.locator('button.badge').first();
    await b.click();
    check('M6: badge is a disclosure with a visible sentence', (await b.getAttribute('aria-expanded')) === 'true' && (await page.locator('.badge-sentence:not([hidden])').count()) >= 1);
  }

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
  // M10: a review step shows what becomes public before signing.
  await page.waitForSelector('#form-review-sign');
  check('M10: review step before signing', (await page.locator('#form-review .review-panel h2').textContent()) === 'This will be public' && (await page.locator('#form-review .notice-good').count()) === 0);
  await page.locator('#form-review-sign').click();
  await page.waitForSelector('#form-review .notice-good');
  check('review published', true);

  // M9: every problem at once, tied to the fields.
  await go(page, '#/publish?kind=listing');
  await page.waitForSelector('#form-listing');
  await page.locator('#form-listing button[type=submit]').click();
  await page.waitForSelector('#form-listing .error-summary');
  const nErr = await page.locator('#form-listing .error-summary li').count();
  const nInvalid = await page.locator('#form-listing [aria-invalid=true]').count();
  check('M9: all form errors listed and marked', nErr >= 3 && nInvalid >= 3, `${nErr} errors, ${nInvalid} fields`);

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
  // A product whose every description is hidden matches only by GTIN.
  await go(page, '#/search?q=6901234567892');
  await page.waitForSelector('.moderated-hits');
  check('search groups hits with hidden product records', (await page.locator('.moderated-hits .hit').count()) === 1);
  // Follow-up (1): products without a verified source form their own group.
  await go(page, '#/search?q=kestrel');
  await page.waitForSelector('[data-group=unverified] .hit');
  const groups = await page.locator('.hit').evaluateAll((els) => els.map((e) => e.closest('[data-group]')?.dataset.group));
  check('verified-source products come first, then the labeled unverified group', groups[0] === 'verified' && groups.at(-1) === 'unverified' && groups.indexOf('unverified') > groups.lastIndexOf('verified'), groups.join(','));
  check('unverified group is labeled', (await page.locator('#h-unverified-hits').textContent()).startsWith('No verified description'));
  check('ranking disclosure says verified sources come first', (await page.locator('.computed summary').first().textContent()) === 'How ranking works' && (await page.locator('.computed').first().textContent()).includes('verified source always come first'));
  await go(page, '#/search?q=iphone');
  await page.waitForSelector('.screen-search .empty');
  check('an unverified key cannot add words to a product with a verified source', (await page.locator('.hit').count()) === 0);
  // Follow-up (3): one hidden description of a visible product.
  await go(page, `#/product/${encodeURIComponent('gtin:05012345678900')}`);
  await page.waitForSelector('.screen-product');
  {
    const n = page.locator('[data-status=hidden-descriptions]');
    check('hidden description shown as a count', (await n.count()) === 1 && (await n.locator('.notice-title').textContent()) === '1 description of this product was hidden by moderation');
    await n.locator('.flag-list summary').click();
    await page.waitForSelector('[data-status=hidden-descriptions] .description-flags .flags li');
    check('hidden description lists its flags', (await n.locator('.flags li').count()) === 5 && !(await n.textContent()).includes('True Wireless'));
    check('hidden description gives no facts', (await page.locator('h1').textContent()) === 'Aster A40 wireless earbuds');
  }
  // Follow-up (2): a receipt that cannot be checked here is neutral.
  await go(page, `#/product/${encodeURIComponent('gtin:07612345678900')}`);
  await page.waitForSelector('.screen-product');
  {
    const r = page.locator('.review[data-purchase=listing_unavailable]');
    const b = r.locator('p.badges .badge').first();
    check('listing_unavailable receipt shown in neutral color', (await r.count()) === 1 && (await b.textContent()).includes("Receipt can't be checked") && (await b.getAttribute('class')).includes('badge-muted'));
  }
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
  const shownStars = await page.locator('.review:not([hidden]) .star-text').evaluateAll((els) => els.map((e) => e.textContent));
  check('histogram button filters reviews by stars', shownStars.length > 0 && shownStars.every((l) => l === '5 of 5'), shownStars.join('; '));
  check('star filter stored in the URL', (await page.evaluate(() => location.hash)).includes('stars=5'));
  check('m3: identity histogram also filters to identity-verified people', (await page.evaluate(() => location.hash)).includes('reviews=identity'));
  check('M12: stars show the number as text', /^[1-5] of 5$/.test(await page.locator('.review .star-text').first().textContent()));
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
  const dark = await newPage('dark', { nodes: ['ws://mock-a/v1/sync'] });
  await dark.page.setViewportSize({ width: 375, height: 800 });
  await dark.page.goto(`${url}/?mock=1#/product/${encodeURIComponent('gtin:04006381333931')}`);
  await waitReady(dark.page);
  await dark.page.waitForSelector('[data-banner=one-node]');
  check('M3: one-node banner shown', (await dark.page.locator('[data-banner=one-node]').textContent()).includes('Connected to 1 node'));
  check('M3: nav status is text', (await dark.page.locator('#nav-net').textContent()) === '1 node');
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

  // Audit #13: ?nodes= adds nodes for this session only, with a banner.
  {
    const s = await newPage('light', { nodes: ['ws://mock-a/v1/sync'] });
    await s.page.goto(`${url}/?mock=1&nodes=ws://mock-x/v1/sync#/network`);
    await waitReady(s.page);
    await s.page.waitForSelector('#peers-table');
    const banner = s.page.locator('#session-nodes-banner');
    check('?nodes= shows a session banner', (await banner.isVisible()) && (await banner.textContent()).includes('ws://mock-x/v1/sync'));
    check('?nodes= keeps the saved nodes', (await s.page.locator('#peers-table tbody tr').count()) === 2 && (await s.page.locator('#peers-table .session-badge').count()) === 1);
    await s.page.locator('#session-nodes-discard').press('Enter');
    await s.page.waitForFunction(() => !document.getElementById('session-nodes-banner'));
    await s.page.waitForTimeout(500);
    check('Discard drops the session node', (await s.page.locator('#peers-table tbody tr').count()) === 1 && !(await s.page.evaluate(() => location.search.includes('nodes='))));
    await s.page.goto(`${url}/?mock=1&nodes=ws://mock-y/v1/sync`);
    await waitReady(s.page);
    await s.page.locator('#session-nodes-keep').press('Enter');
    const saved = await s.page.evaluate(() => JSON.parse(localStorage.getItem('perch.settings.v1')).nodes);
    check('Keep saves the session node', JSON.stringify(saved) === JSON.stringify(['ws://mock-a/v1/sync', 'ws://mock-y/v1/sync']), JSON.stringify(saved));
    await s.ctx.close();
  }

  // Audit #4: a node that claims records it does not send blocks publishing
  // until the user stops using it. Retry keeps it blocked.
  {
    const g = await newPage('light', { nodes: ['ws://mock-a/v1/sync', 'ws://mock-withhold/v1/sync'], claimTimeoutMs: 1500 });
    await g.page.goto(`${url}/?mock=1#/keys`);
    await waitReady(g.page);
    await g.page.waitForTimeout(500);
    await g.page.locator('#generate-key').click();
    await g.page.waitForSelector('#my-author');
    await g.page.locator('#backup-ok').check();
    await go(g.page, '#/publish?kind=profile');
    await g.page.waitForSelector('#form-profile');
    const gate = g.page.locator('.screen-publish > .notice-warn');
    check('withholding node: publishing waits', (await gate.textContent()).includes('Waiting for the node to send the records'));
    await g.page.waitForSelector('.screen-publish > .notice-warn [data-action=stop]', { timeout: 8000 });
    const text = await gate.textContent();
    check('after the timeout the node is named with Retry and Stop', text.includes('mock-withhold') && text.includes('Retry, or stop using this node') && (await gate.locator('[data-action=retry]').count()) === 1);
    await gate.locator('[data-action=retry]').press('Enter');
    await g.page.waitForTimeout(300);
    check('Retry keeps publishing paused', (await g.page.locator('.screen-publish > .notice-warn').count()) === 1 && (await g.page.locator('.screen-publish > .notice-warn [data-action=stop]').count()) === 0);
    await g.page.fill('#f-name', 'Guard test');
    await g.page.locator('#form-profile button[type=submit]').press('Enter');
    await g.page.waitForSelector('#form-profile .form-status .notice');
    check('submit while paused does not publish', (await g.page.locator('#form-profile .notice-good').count()) === 0);
    await g.page.waitForSelector('.screen-publish > .notice-warn [data-action=stop]', { timeout: 8000 });
    await g.page.locator('.screen-publish > .notice-warn [data-action=stop]').press('Enter');
    await g.page.waitForFunction(() => !document.querySelector('.screen-publish > .notice-warn'), null, { timeout: 5000 });
    const savedNodes = await g.page.evaluate(() => JSON.parse(localStorage.getItem('perch.settings.v1')).nodes);
    check('Stop using this node unblocks and removes it', JSON.stringify(savedNodes) === JSON.stringify(['ws://mock-a/v1/sync']), JSON.stringify(savedNodes));
    await g.page.fill('#f-name', 'Guard test');
    await g.page.locator('#form-profile button[type=submit]').press('Enter');
    await g.page.locator('#form-profile-sign').press('Enter');
    await g.page.waitForSelector('#form-profile .notice-good');
    check('publishing works after stopping the node', true);
    await g.ctx.close();
  }

  // Audit #11: an invalid saved policy falls back with a notice.
  {
    const p = await newPage('light', { nodes: ['ws://mock-a/v1/sync'] });
    await p.ctx.addInitScript(() => {
      try {
        localStorage.setItem('perch.policy.v1.mock', JSON.stringify({ flag_threshold: 2 ** 60, trusted_attestors: [] }));
      } catch {
        // ignore
      }
    });
    await p.page.goto(`${url}/?mock=1#/policy`);
    await waitReady(p.page);
    const warn = p.page.locator('#banner .banner-warn');
    check('invalid saved policy falls back with a notice', (await warn.count()) === 1 && (await warn.textContent()).includes('bundled policy'));
    await p.page.fill('#p-flag_threshold', '9007199254740992');
    await p.page.locator('#policy-form button[type=submit]').click();
    check('policy screen refuses integers above 2^53 - 1', ((await p.page.locator('#policy-form [role=status] .notice-bad').textContent()) || '').includes('9007199254740991'));
    await p.page.fill('#p-flag_threshold', '3');
    if ((await p.page.locator('.attestor-row').count()) === 0) await p.page.getByRole('button', { name: 'Add attestor' }).click();
    await p.page.locator('.attestor-row [name=att-author]').first().fill('ab'.repeat(32));
    await p.page.locator('.attestor-row [name=att-cutoff]').first().fill('9007199254740993');
    await p.page.locator('#policy-form button[type=submit]').click();
    check('policy screen refuses a cutoff above 2^53 - 1', ((await p.page.locator('#policy-form [role=status] .notice-bad').textContent()) || '').includes('Ignore records after #'));
    await p.ctx.close();
  }
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
