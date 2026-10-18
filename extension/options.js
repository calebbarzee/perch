import { loadSettings, DEFAULT_SETTINGS } from './src/client.js';

const form = document.getElementById('form');
const nodesEl = document.getElementById('nodes');
const appEl = document.getElementById('app');
const status = document.getElementById('status');

function parseNodes(text) {
  const out = [];
  for (const line of text.split('\n').map((s) => s.trim()).filter(Boolean)) {
    let u;
    try {
      u = new URL(line);
    } catch {
      throw new Error(`"${line}" is not a URL.`);
    }
    if (u.protocol !== 'http:' && u.protocol !== 'https:') throw new Error(`"${line}" must start with http:// or https://.`);
    out.push(u.origin);
  }
  return [...new Set(out)];
}

// Nodes other than localhost need a host permission, granted by the user.
async function requestHosts(nodes) {
  const origins = nodes
    .map((n) => new URL(n))
    .filter((u) => u.hostname !== '127.0.0.1' && u.hostname !== 'localhost')
    .map((u) => `${u.protocol}//${u.hostname}/*`);
  if (!origins.length) return true;
  return chrome.permissions.request({ origins });
}

async function init() {
  const s = await loadSettings();
  nodesEl.value = s.nodes.join('\n');
  appEl.value = s.appUrl;
}

form.addEventListener('submit', async (ev) => {
  ev.preventDefault();
  status.textContent = '';
  try {
    const nodes = parseNodes(nodesEl.value);
    const appUrl = appEl.value.trim();
    if (appUrl) new URL(appUrl);
    const granted = await requestHosts(nodes.length ? nodes : DEFAULT_SETTINGS.nodes);
    if (!granted) throw new Error('Permission to contact these nodes was not granted.');
    await chrome.storage.local.set({ nodes: nodes.length ? nodes : [...DEFAULT_SETTINGS.nodes], appUrl });
    status.textContent = 'Saved.';
  } catch (e) {
    status.textContent = e.message;
  }
});

init();
