// App shell: boots the client, restores device state, routes, and renders.

import { loadClient, APP_VERSION } from './client.js';
import { h, clear, announce } from './dom.js';
import { route, parseHash } from './router.js';
import { configuredNodes, settings } from './settings.js';
import * as persist from './persist.js';
import { hex, notice, setNameLookup } from './components.js';
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
  flag_thresholds: { review: 3 },
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

async function boot() {
  const main = document.getElementById('main');
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

  setNameLookup((id) => app.client.authorView(id)?.name ?? null);

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
      h('div', {}, h('dt', { text: 'Wasm SHA-256' }), h('dd', {}, wasm ? hex(wasm, { n: 16 }) : h('span', { text: app.mode === 'mock' ? 'none (mock client)' : 'unknown' }))),
      h('div', {}, h('dt', { text: 'Policy hash' }), h('dd', {}, h('a', { href: '#/policy' }, hex(policyHash, { n: 16 })))),
    ),
    h('p', { class: 'footer-note' }, 'Views are computed on this device from signed records. ', h('a', { href: '#/data', text: 'Inspect the data' }), '.'),
  );
  updateNavStatus();
}

function updateNavStatus() {
  const el = document.getElementById('nav-net');
  if (!el) return;
  let peers = [];
  try {
    peers = app.client.peers();
  } catch {
    peers = [];
  }
  const live = peers.filter((p) => p.state === 'live').length;
  el.textContent = peers.length ? `${live}/${peers.length}` : '0';
  el.dataset.state = !peers.length ? 'none' : live === peers.length ? 'ok' : live ? 'partial' : 'down';
  const link = el.closest('a');
  if (link) link.setAttribute('aria-label', `Network, ${live} of ${peers.length} nodes connected`);
}

function wireTopbar() {
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
