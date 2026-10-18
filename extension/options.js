import { loadSettings, DEFAULT_SETTINGS } from './src/client.js';

const form = document.getElementById('form');
const nodesEl = document.getElementById('nodes');
const appEl = document.getElementById('app');
const status = document.getElementById('status');
const sitesEl = document.getElementById('sites');
const noSites = document.getElementById('no-sites');
let hiddenSites = [];

function parseNodes(text) {
  const out = [];
  for (const line of text.split('\n').map((s) => s.trim()).filter(Boolean)) {
    let u;
    try {
      u = new URL(line);
    } catch {
      throw new Error(`"${line}" is not a web address. Use one like http://127.0.0.1:7700.`);
    }
    if (u.protocol !== 'http:' && u.protocol !== 'https:') throw new Error(`"${line}" must start with http:// or https://.`);
    out.push(u.origin);
  }
  return [...new Set(out)];
}

// Nodes other than this computer need a host permission, granted by the user.
async function requestHosts(nodes) {
  const origins = nodes
    .map((n) => new URL(n))
    .filter((u) => u.hostname !== '127.0.0.1' && u.hostname !== 'localhost')
    .map((u) => `${u.protocol}//${u.hostname}/*`);
  if (!origins.length) return true;
  return chrome.permissions.request({ origins });
}

function renderSites() {
  sitesEl.replaceChildren();
  for (const site of hiddenSites) {
    const li = document.createElement('li');
    const name = document.createElement('code');
    name.textContent = site;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'small';
    btn.textContent = `Show on ${site} again`;
    btn.addEventListener('click', () => {
      hiddenSites = hiddenSites.filter((s) => s !== site);
      renderSites();
    });
    li.append(name, ' ', btn);
    sitesEl.append(li);
  }
  noSites.hidden = hiddenSites.length > 0;
}

async function init() {
  const s = await loadSettings();
  nodesEl.value = s.nodes.join('\n');
  appEl.value = s.appUrl;
  document.getElementById(s.inPage ? 'show-page' : 'show-toolbar').checked = true;
  hiddenSites = s.hiddenSites;
  renderSites();
}

form.addEventListener('submit', async (ev) => {
  ev.preventDefault();
  status.textContent = '';
  try {
    const nodes = parseNodes(nodesEl.value);
    const appUrl = appEl.value.trim();
    if (appUrl) new URL(appUrl);
    const list = nodes.length ? nodes : [...DEFAULT_SETTINGS.nodes];
    const granted = await requestHosts(list);
    if (!granted) throw new Error('Permission to contact these nodes was not granted.');
    const inPage = form.elements.show.value === 'page';
    await chrome.storage.local.set({ nodes: list, appUrl, inPage, hiddenSites, setupDone: true });
    status.textContent = 'Saved.';
  } catch (e) {
    status.textContent = e.message;
  }
});

init();
