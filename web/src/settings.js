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

// Nodes from ?nodes=ws://a,ws://b. They never replace the configured nodes
// (ARCHITECTURE 10): they are added for this page session only, and the app
// shows a banner where the user keeps (saves) or discards them.
export function urlNodes() {
  const params = new URLSearchParams(location.search);
  if (!params.has('nodes')) return null;
  const v = params.get('nodes') || '';
  return [...new Set(v.split(',').map((s) => s.trim()).filter(validNodeUrl))];
}

let session = null; // nodes added for this session, not saved

// Nodes from the address that are not configured yet.
export function sessionNodes() {
  if (session === null) {
    const configured = configuredNodes();
    session = (urlNodes() || []).filter((u) => !configured.includes(u));
  }
  return session;
}

// Saves the session nodes to the configured list.
export function keepSessionNodes() {
  const add = sessionNodes();
  const list = configuredNodes();
  saveSettings({ nodes: [...list, ...add.filter((u) => !list.includes(u))] });
  session = [];
  dropNodesParam();
}

// Forgets the session nodes; returns them so the caller can disconnect.
export function discardSessionNodes() {
  const dropped = sessionNodes();
  session = [];
  dropNodesParam();
  return dropped;
}

// "Stop using this node": removes it from the session or the saved list.
export function forgetNode(url) {
  if (sessionNodes().includes(url)) {
    session = session.filter((u) => u !== url);
    return;
  }
  saveSettings({ nodes: configuredNodes().filter((u) => u !== url) });
}

function dropNodesParam() {
  try {
    const u = new URL(location.href);
    if (!u.searchParams.has('nodes')) return;
    u.searchParams.delete('nodes');
    history.replaceState(history.state, '', u.href);
  } catch {
    // ignore
  }
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

// The saved node list, or the release default.
export function configuredNodes() {
  return settings().nodes ?? defaultNodes();
}

// Every node this page connects to: configured plus session nodes.
export function activeNodes() {
  return [...configuredNodes(), ...sessionNodes()];
}

export function validNodeUrl(url) {
  try {
    const u = new URL(url);
    return (u.protocol === 'ws:' || u.protocol === 'wss:') && Boolean(u.host);
  } catch {
    return false;
  }
}
