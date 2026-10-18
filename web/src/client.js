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

async function loadWasm() {
  const url = new URL('../pkg/perch_wasm.js', import.meta.url);
  const mod = await import(url.href);
  if (typeof mod.default === 'function') await mod.default();
  if (!mod.PerchClient) throw new Error('web/pkg/perch_wasm.js does not export PerchClient');
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
  const client = await Impl.create(options);
  loaded = { client, mode, reason };
  return loaded;
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
