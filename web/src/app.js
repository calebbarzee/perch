// App shell: boots the client, restores device state, routes, and renders.

import { loadClient, APP_VERSION } from './client.js';
import { h, clear, announce } from './dom.js';
import { route, parseHash } from './router.js';
import { configuredNodes, settings, setConfig, appConfig } from './settings.js';
import * as persist from './persist.js';
import { hex, notice, setAuthorLookup, clearAuthorCache } from './components.js';
import { setCurrencyTable } from './format.js';

import search from './screens/search.js';
import product from './screens/product.js';
import listing from './screens/listing.js';
import author from './screens/author.js';
import publish from './screens/publish.js';
import keys from './screens/keys.js';
import network from './screens/network.js';
import policy from './screens/policy.js';
import data from './screens/data.js';
import notFound from './screens/not-found.js';

route('/search', search);
route('/product/:key', product);
route('/listing/:id', listing);
route('/author/:id', author);
route('/publish', publish);
route('/keys', keys);
route('/network', network);
route('/policy', policy);
route('/data', data);
route('/data/:id', data);

const POLICY_KEY = 'perch.policy.v1';

// Used only if web/default-policy.json cannot be read.
const FALLBACK_POLICY = Object.freeze({
  now: null,
  trusted_attestors: [],
  flag_threshold: 3,
  flag_thresholds: { review: 5, product: 5 },
  flags_require_verified: true,
  seller_hidden_listing_limit: 3,
  seller_min_distinct_flaggers: 9,
  verified_purchase_requires_verified_buyer: true,
  price_requires_domain_verified: true,
  future_ts_tolerance_ms: 600000,
});

export const app = {
  client: null,
  mode: null,
  reason: null,
  build: null,
  bundledPolicy: null,
  keepKey: false,
  restoredKey: false,
  bootErrors: [],
};

// Policy: the bundled default unless the user saved their own.
function loadPolicy() {
  try {
    const raw = localStorage.getItem(`${POLICY_KEY}.${app.mode}`);
    if (raw) return JSON.parse(raw);
  } catch {
    // ignore
  }
  return structuredClone(app.bundledPolicy);
}

export function savePolicy(policy) {
  app.client.setPolicy(policy);
  try {
    localStorage.setItem(`${POLICY_KEY}.${app.mode}`, JSON.stringify(policy));
  } catch {
    // ignore
  }
}

export function resetPolicy() {
  try {
    localStorage.removeItem(`${POLICY_KEY}.${app.mode}`);
  } catch {
    // ignore
  }
  app.client.setPolicy(structuredClone(app.bundledPolicy));
}

// Key persistence after generate/import/publish.
export async function persistKey() {
  const c = app.client;
  const a = c.author();
  if (!a) return;
  const backup = c.exportKey();
  try {
    const head = JSON.parse(backup).head;
    if (head) persist.rememberHead(a, head);
  } catch {
    // ignore
  }
  if (app.keepKey) await persist.saveKey(backup);
}

export async function setKeepKey(keep) {
  app.keepKey = keep;
  try {
    localStorage.setItem(`perch.keepKey.${app.mode}`, keep ? '1' : '0');
  } catch {
    // ignore
  }
  if (keep) await persistKey();
  else await persist.forgetKey();
}

async function loadJson(rel) {
  try {
    const res = await fetch(new URL(rel, import.meta.url), { cache: 'no-store' });
    return res.ok ? await res.json() : null;
  } catch {
    return null;
  }
}

// Compares the running wasm hash with pkg/SHA256SUMS shipped with the build.
async function loadChecksum() {
  const url = new URL('../pkg/SHA256SUMS', import.meta.url).href;
  const out = { url, expected: null, state: 'missing' };
  if (app.mode !== 'wasm' || !app.build?.wasm_sha256) return { ...out, state: 'none' };
  try {
    const res = await fetch(url, { cache: 'no-store' });
    if (!res.ok) return out;
    const line = (await res.text()).split('\n').find((l) => /\sperch_wasm_bg\.wasm$/.test(l.trim()));
    if (!line) return out;
    out.expected = line.trim().split(/\s+/)[0];
    out.state = out.expected === app.build.wasm_sha256 ? 'match' : 'mismatch';
  } catch {
    // stays 'missing'
  }
  return out;
}

async function boot() {
  const main = document.getElementById('main');
  setConfig(await loadJson('../config.json'));
  let loaded;
  try {
    loaded = await loadClient();
  } catch (e) {
    clear(main).append(notice('bad', 'The Perch client did not start.', h('p', { text: String(e.message || e) })));
    return;
  }
  app.client = loaded.client;
  app.mode = loaded.mode;
  app.reason = loaded.reason;
  document.documentElement.dataset.client = app.mode;
  persist.useNamespace(app.mode);
  try {
    setCurrencyTable(app.client.currencies());
  } catch (e) {
    app.bootErrors.push(`The currency table did not load (${e.message}). Prices show in minor units.`);
  }
  try {
    app.build = app.client.buildInfo();
  } catch {
    app.build = { version: null, wasm_sha256: null };
  }
  app.checksum = await loadChecksum();

  // The bundled policy is a static file shipped with this release
  // (ARCHITECTURE 10). The mock client brings its own, because its demo
  // attestor key differs from the real demo dataset.
  if (app.mode === 'mock') {
    app.bundledPolicy = app.client.policy();
  } else {
    try {
      const res = await fetch(new URL('../default-policy.json', import.meta.url), { cache: 'no-store' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      app.bundledPolicy = await res.json();
    } catch (e) {
      app.bootErrors.push(`The bundled policy file did not load (${e.message}). No attestors are trusted until you add them on the Policy screen.`);
      app.bundledPolicy = { ...FALLBACK_POLICY };
    }
  }
  try {
    app.client.setPolicy(loadPolicy());
  } catch (e) {
    app.bootErrors.push(`Saved policy rejected (${e.message}); using the bundled policy.`);
    app.client.setPolicy(structuredClone(app.bundledPolicy));
  }
  const pinned = settings().pinnedNow;
  if (pinned != null) {
    const p = app.client.policy();
    p.now = pinned;
    app.client.setPolicy(p);
  }

  // Restore records, then the key.
  const jsonl = await persist.loadRecords();
  if (jsonl) {
    try {
      const res = app.client.ingest(jsonl);
      if (res.rejected) app.bootErrors.push(`${res.rejected} stored records failed verification and were dropped.`);
    } catch (e) {
      app.bootErrors.push(`Stored records could not be loaded (${e.message}).`);
    }
  }
  try {
    app.keepKey = localStorage.getItem(`perch.keepKey.${app.mode}`) !== '0';
  } catch {
    app.keepKey = true;
  }
  // A restored key counts as imported: another device may have published
  // with it, so the client syncs its log from the nodes before publishing.
  const storedKey = await persist.loadKey();
  if (storedKey) {
    try {
      const author = app.client.importKey(storedKey);
      const merged = persist.mergeBackupHead(storedKey, author);
      if (merged !== storedKey) app.client.importKey(merged);
      app.restoredKey = true;
    } catch (e) {
      app.bootErrors.push(`The stored key could not be loaded (${e.message}).`);
    }
  }

  setAuthorLookup((id) => app.client.authorView(id));

  app.client.onChange((ev) => {
    const reason = ev?.reason ?? 'records';
    if (reason === 'records') persist.scheduleSaveRecords(() => app.client.exportJsonl());
    scheduleLiveUpdate();
  });
  window.addEventListener('pagehide', () => {
    persist.flushRecords(() => app.client.exportJsonl());
  });

  for (const url of configuredNodes()) {
    // Failures show on the Network screen; the client keeps retrying.
    app.client.connect(url).catch(() => {});
  }

  renderBanner();
  renderFooter();
  wireTopbar();
  window.addEventListener('hashchange', () => show({ navigation: true }));
  await show({ navigation: false });
  document.documentElement.dataset.ready = 'true';
}

function renderBanner() {
  const slot = document.getElementById('banner');
  clear(slot);
  if (app.mode === 'mock') {
    slot.append(
      h('div', { class: 'banner banner-mock', role: 'note' },
        h('p', {},
          h('strong', { text: 'Demo data. ' }),
          'This page runs the mock client: signatures are fake and nothing is sent to a real node. ',
          app.reason ? app.reason : null,
        ),
      ),
    );
  }
  for (const msg of app.bootErrors) slot.append(h('div', { class: 'banner banner-warn', role: 'alert' }, h('p', { text: msg })));
}

export function renderFooter() {
  const el = document.getElementById('footer');
  const c = app.client;
  let policyHash = '';
  try {
    policyHash = c.policyHash();
  } catch {
    policyHash = '';
  }
  const wasm = app.build?.wasm_sha256;
  clear(el).append(
    h('dl', { class: 'build-info' },
      h('div', {}, h('dt', { text: 'App' }), h('dd', { text: `Perch ${APP_VERSION}${app.mode === 'mock' ? ' (mock client)' : ''}` })),
      h('div', {}, h('dt', { text: 'Wasm SHA-256' }), h('dd', {}, wasm ? [hex(wasm, { n: 16 }), ' ', checksumNote(true)] : h('span', { text: app.mode === 'mock' ? 'none (mock client)' : 'unknown' }))),
      h('div', {}, h('dt', { text: 'Policy hash' }), h('dd', {}, h('a', { href: '#/policy' }, hex(policyHash, { n: 16 })))),
    ),
    h('p', { class: 'footer-note' }, 'Views are computed on this device from signed records. ', h('a', { href: '#/data', text: 'Inspect the data' }), '.'),
  );
  updateNavStatus();
}

// "matches/does not match the checksum file", with links (docs/ux.md T8, 8.7).
export function checksumNote(short = false) {
  const c = app.checksum;
  if (!c || c.state === 'none') return null;
  const cfg = appConfig();
  const fileLink = h('a', { href: c.url, target: '_blank', rel: 'noopener', text: 'SHA256SUMS' });
  const release = cfg.release_page_url ? h('a', { href: cfg.release_page_url, target: '_blank', rel: 'noopener noreferrer', text: 'release page' }) : null;
  const parts = [];
  if (c.state === 'match') parts.push(h('span', { class: 'good-text', dataset: { checksum: 'match' } }, 'matches ', fileLink));
  else if (c.state === 'mismatch') parts.push(h('span', { class: 'bad-text', dataset: { checksum: 'mismatch' } }, 'does not match ', fileLink, '. This app is not the build it claims to be.'));
  else parts.push(h('span', { class: 'warn-text', dataset: { checksum: 'missing' } }, 'no checksum file found'));
  if (release) parts.push(short ? [' (compare with the ', release, ')'] : [' Compare with the checksums on the ', release, '. If they differ, this app is not the published release.']);
  else if (!short) parts.push(' The checksum file comes from the same server as the app. Compare the hash with one published by the release authors.');
  return h('span', { class: 'checksum-note' }, ...parts);
}

function updateNavStatus() {
  const el = document.getElementById('nav-net');
  if (!el) return;
  const menuDot = document.getElementById('nav-net-menu');
  let peers = [];
  try {
    peers = app.client.peers();
  } catch {
    peers = [];
  }
  const live = peers.filter((p) => p.state === 'live').length;
  el.textContent = peers.length ? `${live}/${peers.length}` : '0';
  el.dataset.state = !peers.length ? 'none' : live === peers.length ? 'ok' : live ? 'partial' : 'down';
  if (menuDot) {
    menuDot.textContent = el.textContent;
    menuDot.dataset.state = el.dataset.state;
  }
  const link = el.closest('a');
  if (link) link.setAttribute('aria-label', `Network, ${live} of ${peers.length} nodes connected`);
}

function wireTopbar() {
  // Below 600 px the nav is a disclosure (docs/ux.md 5.2).
  const nav = document.getElementById('topnav');
  const menu = document.getElementById('menu-button');
  const setOpen = (open) => {
    nav.classList.toggle('open', open);
    menu.setAttribute('aria-expanded', String(open));
  };
  menu?.addEventListener('click', () => setOpen(!nav.classList.contains('open')));
  nav?.addEventListener('keydown', (ev) => {
    if (ev.key === 'Escape' && nav.classList.contains('open')) {
      setOpen(false);
      menu.focus();
    }
  });
  window.addEventListener('hashchange', () => setOpen(false));
  // The hash is the router, so the skip link moves focus itself.
  document.getElementById('skip-link')?.addEventListener('click', (ev) => {
    ev.preventDefault();
    const main = document.getElementById('main');
    const target = main.querySelector('h1') || main;
    target.setAttribute('tabindex', '-1');
    target.focus();
  });
  const form = document.getElementById('topbar-search');
  form.addEventListener('submit', (ev) => {
    ev.preventDefault();
    const q = form.elements.q.value.trim();
    const { params } = parseHash();
    const p = new URLSearchParams();
    if (q) p.set('q', q);
    if (params.sort) p.set('sort', params.sort);
    location.hash = '#/search' + (p.toString() ? `?${p}` : '');
  });
}

// Screen rendering.

let current = null; // { screen, ctx, signature }
let renderSeq = 0;

function makeCtx(parsed) {
  const controller = new AbortController();
  const subs = [];
  return {
    client: app.client,
    app,
    args: parsed.args,
    params: parsed.params,
    path: parsed.path,
    signal: controller.signal,
    on(fn) {
      subs.push(app.client.onChange(fn));
    },
    refresh: () => show({ navigation: false, refresh: true }),
    dispose() {
      controller.abort();
      for (const u of subs) u();
    },
  };
}

async function show({ navigation, refresh = false }) {
  const main = document.getElementById('main');
  const parsed = parseHash();
  const screen = parsed.screen || notFound;
  const seq = ++renderSeq;
  const scroll = refresh ? window.scrollY : 0;
  const focusKey = refresh ? document.activeElement?.dataset?.focusKey || document.activeElement?.id : null;

  if (current) current.ctx.dispose();
  clearAuthorCache();
  const ctx = makeCtx(parsed);
  let node;
  try {
    node = await screen.render(ctx);
  } catch (e) {
    console.error(e);
    node = h('section', {}, h('h1', { tabindex: '-1', text: 'Something went wrong' }), notice('bad', 'This screen failed to render.', h('p', { text: String(e.message || e) })));
  }
  if (seq !== renderSeq) return;
  current = { screen, ctx, signature: liveSignature(screen, ctx) };
  main.replaceChildren(node);

  const titleText = node.querySelector('h1')?.textContent?.trim();
  document.title = titleText ? `${titleText} – Perch` : 'Perch';
  for (const a of document.querySelectorAll('[data-nav]')) {
    if (a.dataset.nav === (screen.nav || screen.id)) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  }
  const q = document.getElementById('topbar-q');
  if (q && screen.id === 'search') q.value = parsed.params.q || '';

  if (refresh) {
    window.scrollTo(0, scroll);
    if (focusKey) {
      const el = main.querySelector(`[data-focus-key="${CSS.escape(focusKey)}"]`) || document.getElementById(focusKey);
      el?.focus({ preventScroll: true });
    }
  } else if (navigation) {
    window.scrollTo(0, 0);
    const h1 = main.querySelector('h1');
    if (h1) {
      h1.setAttribute('tabindex', '-1');
      h1.focus({ preventScroll: true });
    }
    if (titleText) announce(titleText);
  }
}

function liveSignature(screen, ctx) {
  if (!screen.live) return null;
  try {
    return screen.live(ctx);
  } catch {
    return null;
  }
}

let liveTimer = null;
function scheduleLiveUpdate() {
  if (liveTimer) return;
  liveTimer = setTimeout(() => {
    liveTimer = null;
    renderFooter();
    if (!current || !current.screen.live) return;
    // Do not re-render while the user is typing in this screen.
    const active = document.activeElement;
    if (active && /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName) && document.getElementById('main').contains(active)) return;
    const sig = liveSignature(current.screen, current.ctx);
    if (sig !== current.signature) show({ navigation: false, refresh: true });
  }, 400);
}

boot();
