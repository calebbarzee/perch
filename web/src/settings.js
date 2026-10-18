// Per-device settings in localStorage: node list and display preferences.
// The key and records are stored separately (persist.js).

const KEY = 'perch.settings.v1';

const defaults = () => ({
  nodes: null, // null = use the default for this page (see defaultNodes)
  autoLoadVerifiedImages: false,
  backupConfirmedFor: null, // author hex whose backup the user confirmed
  pinnedNow: null,
});

let cache = null;

export function settings() {
  if (cache) return cache;
  cache = defaults();
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) Object.assign(cache, JSON.parse(raw));
  } catch {
    // Storage blocked: keep defaults for this session.
  }
  return cache;
}

export function saveSettings(patch) {
  Object.assign(settings(), patch);
  try {
    localStorage.setItem(KEY, JSON.stringify(cache));
  } catch {
    // Ignore: settings then last for this session only.
  }
}

// Nodes from ?nodes=ws://a,ws://b override stored ones for this page load.
export function urlNodes() {
  const params = new URLSearchParams(location.search);
  if (!params.has('nodes')) return null;
  const v = params.get('nodes') || '';
  return v.split(',').map((s) => s.trim()).filter(Boolean);
}

// Release configuration from web/config.json (set by the app at boot).
// bootstrap_nodes: node sync addresses; a relative path such as "/v1/sync"
// means the node that serves this page. release_page_url: where the release
// publishes its checksums; empty hides the link.
let config = { bootstrap_nodes: ['/v1/sync'], release_page_url: '' };
export function setConfig(c) {
  if (c && typeof c === 'object') config = { ...config, ...c };
}
export function appConfig() {
  return config;
}

export function defaultNodes() {
  const out = [];
  const served = location.protocol === 'http:' || location.protocol === 'https:';
  for (const entry of config.bootstrap_nodes || []) {
    if (typeof entry !== 'string') continue;
    if (/^wss?:\/\//.test(entry)) {
      out.push(entry);
    } else if (served && entry.startsWith('/')) {
      const ws = location.protocol === 'https:' ? 'wss:' : 'ws:';
      out.push(`${ws}//${location.host}${entry}`);
    }
  }
  return [...new Set(out)];
}

export function configuredNodes() {
  return urlNodes() ?? settings().nodes ?? defaultNodes();
}

export function validNodeUrl(url) {
  try {
    const u = new URL(url);
    return (u.protocol === 'ws:' || u.protocol === 'wss:') && Boolean(u.host);
  } catch {
    return false;
  }
}
