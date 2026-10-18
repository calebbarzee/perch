// Loads the Perch wasm client bundled in extension/pkg (built by
// scripts/build-wasm.sh). The extension holds no key: it creates only sparse
// clients, which refuse to publish.

let modPromise = null;
let wasmSha256 = null;

export async function sha256Hex(data) {
  const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
  const d = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return Array.from(d, (b) => b.toString(16).padStart(2, '0')).join('');
}

async function load() {
  const mod = await import(chrome.runtime.getURL('pkg/perch_wasm.js'));
  // A missing extension file makes fetch throw rather than return 404.
  const res = await fetch(chrome.runtime.getURL('pkg/perch_wasm_bg.wasm')).catch(() => null);
  if (res?.ok) {
    const bytes = new Uint8Array(await res.arrayBuffer());
    wasmSha256 = await sha256Hex(bytes);
    await mod.default({ module_or_path: bytes });
    if (typeof mod.setWasmSha256 === 'function') mod.setWasmSha256(wasmSha256);
  } else {
    // Test builds may replace the package with a JavaScript client.
    await mod.default?.();
  }
  if (!mod.PerchClient) throw new Error('pkg/perch_wasm.js does not export PerchClient');
  return mod;
}

export async function perchClientClass() {
  modPromise ??= load();
  return (await modPromise).PerchClient;
}

export function loadedWasmSha256() {
  return wasmSha256;
}

export async function bundledPolicy() {
  const res = await fetch(chrome.runtime.getURL('default-policy.json'));
  if (!res.ok) throw new Error(`default-policy.json: HTTP ${res.status}`);
  return res.json();
}

export const DEFAULT_SETTINGS = Object.freeze({
  nodes: ['http://127.0.0.1:7700'],
  appUrl: '',
});

export async function loadSettings() {
  const s = await chrome.storage.local.get(DEFAULT_SETTINGS);
  const nodes = Array.isArray(s.nodes) ? s.nodes.filter((n) => typeof n === 'string' && n) : [];
  return { nodes: nodes.length ? nodes : [...DEFAULT_SETTINGS.nodes], appUrl: typeof s.appUrl === 'string' ? s.appUrl : '' };
}

// The web app link: the configured app URL, or the first node (nodes serve
// the app at their root).
export function appProductUrl(settings, key) {
  const base = settings.appUrl || (settings.nodes[0] ? new URL('/', settings.nodes[0]).href : '');
  if (!base) return null;
  const u = new URL(base);
  u.hash = `#/product/${encodeURIComponent(key)}`;
  return u.href;
}

export function nodeLabel(nodeUrl) {
  try {
    return new URL(nodeUrl).host;
  } catch {
    return nodeUrl;
  }
}
