// The only module that knows where PerchClient comes from.
//
// It loads the wasm client from web/pkg/perch_wasm.js. It falls back to the
// mock client (web/src/mock-client.js) when the package is missing, fails to
// load, or the URL has ?mock=1. Screens import getClient() and never touch the
// implementation directly.

export const APP_VERSION = '0.1.0';

let loaded = null;

function wantMock() {
  const params = new URLSearchParams(location.search);
  return params.get('mock') === '1';
}

// Loads the wasm package. The loader fetches the module bytes itself,
// hashes them, and instantiates exactly those bytes, so the SHA-256 shown
// to the user (ARCHITECTURE 10) describes the code that runs.
export async function loadWasmModule(pkgBase = new URL('../pkg/', import.meta.url)) {
  const mod = await import(new URL('perch_wasm.js', pkgBase).href);
  const res = await fetch(new URL('perch_wasm_bg.wasm', pkgBase), { cache: 'no-store' });
  if (!res.ok) throw new Error(`perch_wasm_bg.wasm: HTTP ${res.status}`);
  const bytes = new Uint8Array(await res.arrayBuffer());
  const digest = await sha256Hex(bytes);
  await mod.default({ module_or_path: bytes });
  if (!mod.PerchClient) throw new Error('web/pkg/perch_wasm.js does not export PerchClient');
  if (typeof mod.setWasmSha256 === 'function') mod.setWasmSha256(digest);
  return mod;
}

async function sha256Hex(bytes) {
  // Web Crypto exists only in secure contexts (https, localhost, 127.0.0.1).
  if (globalThis.crypto?.subtle) {
    const d = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
    return Array.from(d, (b) => b.toString(16).padStart(2, '0')).join('');
  }
  const { sha256Hex: jsSha256 } = await import('./sha256.js');
  return jsSha256(bytes);
}

async function loadWasm() {
  const mod = await loadWasmModule();
  return mod.PerchClient;
}

async function loadMock() {
  const mod = await import('./mock-client.js');
  return mod.PerchClient;
}

// Returns { client, mode: 'wasm' | 'mock', reason }.
export async function loadClient(options = {}) {
  if (loaded) return loaded;
  let Impl = null;
  let mode = 'wasm';
  let reason = null;
  if (wantMock()) {
    mode = 'mock';
    reason = 'The URL asks for the mock client (?mock=1).';
  } else {
    try {
      Impl = await loadWasm();
    } catch (e) {
      mode = 'mock';
      reason = `The wasm client did not load (${e.message}).`;
    }
  }
  if (!Impl) Impl = await loadMock();
  let client = await Impl.create(options);
  if (mode === 'wasm') client = guardTraps(client);
  loaded = { client, mode, reason };
  return loaded;
}

// The wasm module aborts on a Rust panic, which throws a
// WebAssembly.RuntimeError. Read-only calls still work afterwards (lib.rs),
// so a trap becomes an ordinary Error for the screen to show, plus a
// "perch-internal-error" event for the app banner.
export const STOPPED = 'The Perch client stopped after an internal error. Reload the page.';

function guardTraps(client) {
  const report = (error) => window.dispatchEvent(new CustomEvent('perch-internal-error', { detail: { error } }));
  const convert = (e) => {
    if (e instanceof WebAssembly.RuntimeError) {
      const err = new Error(`Internal error in the Perch client (${e.message}). This view is not available.`);
      err.internal = true;
      report(err);
      return err;
    }
    if (e?.message === STOPPED) report(e);
    return e;
  };
  return new Proxy(client, {
    get(target, prop) {
      const v = Reflect.get(target, prop, target);
      if (typeof v !== 'function') return v;
      return (...args) => {
        try {
          const out = v.apply(target, args);
          return out instanceof Promise ? out.catch((e) => Promise.reject(convert(e))) : out;
        } catch (e) {
          throw convert(e);
        }
      };
    },
  });
}

export function getClient() {
  if (!loaded) throw new Error('loadClient() has not finished');
  return loaded.client;
}

export function clientMode() {
  return loaded ? loaded.mode : null;
}

export function clientReason() {
  return loaded ? loaded.reason : null;
}

// ws://host:port/v1/sync -> http://host:port
export function httpBaseFor(wsUrl) {
  try {
    const u = new URL(wsUrl);
    u.protocol = u.protocol === 'wss:' ? 'https:' : 'http:';
    return u.origin;
  } catch {
    return null;
  }
}
