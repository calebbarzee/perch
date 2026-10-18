// Perch content script.
//
// Runs on every page. It reads schema.org Product markup (JSON-LD or
// microdata) and collects the distinct valid GTINs. It makes no network
// requests and stores nothing.
//
// - It tells the background worker how many GTINs it found, for the toolbar
//   button, and answers the popup's request for them.
// - Only if the user turned on "On the page" in options, the page has
//   exactly 1 distinct GTIN, and the site is not on the hidden list, it adds
//   the panel: an extension page in an iframe, collapsed to a small tab. The
//   shop's page cannot read the panel, but it can detect the iframe, so the
//   default is "Toolbar button only" (docs/ux.md 9.2, Q4).

(() => {
  if (window.top !== window) return;
  if (document.documentElement.dataset.perchSeen) return;
  document.documentElement.dataset.perchSeen = '1';

  const GTIN_PROPS = ['gtin', 'gtin14', 'gtin13', 'gtin12', 'gtin8'];
  const PRODUCT_TYPES = new Set(['Product', 'IndividualProduct', 'ProductModel', 'SomeProducts']);
  const TAB = { width: 200, height: 40 };

  // Digits only; spaces and hyphens are common in markup. A GS1 Digital Link
  // URL (https://id.gs1.org/01/<gtin>/...) is read for its GTIN.
  function cleanGtin(value) {
    if (typeof value === 'number') value = String(value);
    if (typeof value !== 'string') return null;
    const link = /\/01\/(\d{8,14})(?:[/?#]|$)/.exec(value);
    const s = link ? link[1] : value.replace(/[\s-]/g, '');
    if (!/^(\d{8}|\d{12}|\d{13}|\d{14})$/.test(s)) return null;
    const digits = s.split('').map(Number);
    const check = digits.pop();
    let sum = 0;
    digits.reverse().forEach((d, i) => {
      sum += d * (i % 2 === 0 ? 3 : 1);
    });
    return (10 - (sum % 10)) % 10 === check ? s.padStart(14, '0') : null;
  }

  function isProductType(t) {
    const list = Array.isArray(t) ? t : [t];
    return list.some((x) => typeof x === 'string' && PRODUCT_TYPES.has(x.replace(/^https?:\/\/schema\.org\//, '')));
  }

  function fromJsonLd(node, out, depth = 0) {
    if (!node || typeof node !== 'object' || depth > 12) return;
    if (Array.isArray(node)) {
      for (const n of node) fromJsonLd(n, out, depth + 1);
      return;
    }
    if (isProductType(node['@type'])) {
      for (const p of GTIN_PROPS) {
        const values = Array.isArray(node[p]) ? node[p] : [node[p]];
        for (const v of values) {
          const g = cleanGtin(v);
          if (g) out.add(g);
        }
      }
    }
    for (const [k, v] of Object.entries(node)) {
      if (k !== '@context' && v && typeof v === 'object') fromJsonLd(v, out, depth + 1);
    }
  }

  function fromMicrodata(doc, out) {
    for (const scope of doc.querySelectorAll('[itemscope][itemtype]')) {
      const types = (scope.getAttribute('itemtype') || '').split(/\s+/).map((t) => t.replace(/^https?:\/\/schema\.org\//, ''));
      if (!types.some((t) => PRODUCT_TYPES.has(t))) continue;
      for (const p of GTIN_PROPS) {
        for (const el of scope.querySelectorAll(`[itemprop~="${p}"]`)) {
          const g = cleanGtin(el.getAttribute('content') ?? el.getAttribute('value') ?? el.textContent);
          if (g) out.add(g);
        }
      }
    }
  }

  // Distinct GTINs as 14 digits.
  function findGtins() {
    const found = new Set();
    for (const s of document.querySelectorAll('script[type="application/ld+json"]')) {
      try {
        fromJsonLd(JSON.parse(s.textContent), found);
      } catch {
        // Ignore broken JSON-LD.
      }
    }
    fromMicrodata(document, found);
    return [...found].sort();
  }

  const host = location.hostname; // ASCII (punycode) form
  let gtins = [];
  let frame = null;
  let expanded = false;

  function report() {
    try {
      chrome.runtime.sendMessage({ type: 'perch:detected', gtins }).catch(() => {});
    } catch {
      // Extension reloaded; ignore.
    }
  }

  function applySize() {
    if (!frame) return;
    const st = frame.style;
    if (expanded) {
      st.setProperty('width', 'min(360px, calc(100vw - 32px))', 'important');
      st.setProperty('height', `${Math.min(frame._contentHeight || 320, window.innerHeight - 32)}px`, 'important');
    } else {
      st.setProperty('width', `${TAB.width}px`, 'important');
      st.setProperty('height', `${TAB.height}px`, 'important');
    }
  }

  function setExpanded(value, { remember = true } = {}) {
    expanded = value;
    applySize();
    frame?.contentWindow?.postMessage({ type: 'perch:expanded', value }, '*');
    if (remember) chrome.storage.local.set({ panelExpanded: value }).catch(() => {});
  }

  function showPanel(gtin, startExpanded) {
    frame = document.createElement('iframe');
    const params = new URLSearchParams({ gtin, host, mode: 'tab', expanded: startExpanded ? '1' : '0' });
    frame.src = chrome.runtime.getURL('panel.html') + '#' + params;
    frame.title = 'Perch: product information';
    frame.setAttribute('referrerpolicy', 'no-referrer');
    const st = frame.style;
    st.setProperty('position', 'fixed', 'important');
    st.setProperty('right', '16px', 'important');
    st.setProperty('bottom', '16px', 'important');
    st.setProperty('border', '0', 'important');
    st.setProperty('border-radius', '10px', 'important');
    st.setProperty('box-shadow', '0 2px 12px rgba(0,0,0,0.18)', 'important');
    st.setProperty('z-index', '2147483647', 'important');
    st.setProperty('color-scheme', 'normal', 'important');
    st.setProperty('background', 'transparent', 'important');
    expanded = startExpanded;
    applySize();
    document.documentElement.append(frame);
    document.documentElement.dataset.perchPanel = 'shown';

    window.addEventListener('message', (ev) => {
      if (!frame || ev.source !== frame.contentWindow || !ev.data || typeof ev.data !== 'object') return;
      const d = ev.data;
      if (d.type === 'perch:height' && Number.isFinite(d.height)) {
        frame._contentHeight = Math.max(60, d.height);
        applySize();
      } else if (d.type === 'perch:expand') {
        setExpanded(true);
      } else if (d.type === 'perch:collapse') {
        setExpanded(false);
      } else if (d.type === 'perch:hide-site') {
        frame.remove();
        frame = null;
        chrome.storage.local.get({ hiddenSites: [] }).then(({ hiddenSites }) => {
          if (!hiddenSites.includes(host)) chrome.storage.local.set({ hiddenSites: [...hiddenSites, host] });
        });
      }
    });
  }

  chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
    if (msg?.type === 'perch:get') {
      reply({ gtins, host });
    } else if (msg?.type === 'perch:toggle' && frame) {
      setExpanded(!expanded);
      reply({ toggled: true });
    } else if (msg?.type === 'perch:toggle') {
      reply({ toggled: false });
    }
    return false;
  });

  async function start() {
    gtins = findGtins();
    report();
    if (gtins.length !== 1 || frame) return;
    const s = await chrome.storage.local.get({ inPage: false, setupDone: false, hiddenSites: [], panelExpanded: false });
    if (!s.inPage || !s.setupDone || s.hiddenSites.includes(host)) return;
    showPanel(gtins[0], Boolean(s.panelExpanded));
  }

  start();
  // Some shops add their markup after load. Look once more.
  setTimeout(() => {
    if (!frame) start();
  }, 1500);
})();
