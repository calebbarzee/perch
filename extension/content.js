// Perch content script.
//
// Runs on every page. It reads schema.org Product markup (JSON-LD or
// microdata) and looks for a GTIN. If it finds a valid one, it adds a small
// panel: an extension page in an iframe. The page cannot read the panel.
//
// This script makes no network requests and stores nothing. The panel does
// the lookup (panel.js).

(() => {
  if (window.top !== window) return;
  if (document.documentElement.dataset.perchPanel) return;

  const GTIN_PROPS = ['gtin', 'gtin14', 'gtin13', 'gtin12', 'gtin8'];
  const PRODUCT_TYPES = new Set(['Product', 'IndividualProduct', 'ProductModel', 'SomeProducts']);

  // Digits only; spaces and hyphens are common in markup.
  function cleanGtin(value) {
    if (typeof value === 'number') value = String(value);
    if (typeof value !== 'string') return null;
    const s = value.replace(/[\s-]/g, '');
    if (!/^(\d{8}|\d{12}|\d{13}|\d{14})$/.test(s)) return null;
    const digits = s.split('').map(Number);
    const check = digits.pop();
    let sum = 0;
    digits.reverse().forEach((d, i) => {
      sum += d * (i % 2 === 0 ? 3 : 1);
    });
    return (10 - (sum % 10)) % 10 === check ? s : null;
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
          if (g) out.push(g);
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
          const v = el.getAttribute('content') ?? el.getAttribute('value') ?? el.textContent;
          const g = cleanGtin(v);
          if (g) out.push(g);
        }
      }
    }
  }

  function findGtin() {
    const found = [];
    for (const s of document.querySelectorAll('script[type="application/ld+json"]')) {
      try {
        fromJsonLd(JSON.parse(s.textContent), found);
      } catch {
        // Ignore broken JSON-LD.
      }
    }
    fromMicrodata(document, found);
    return found[0] ?? null;
  }

  function showPanel(gtin) {
    document.documentElement.dataset.perchPanel = 'shown';
    const frame = document.createElement('iframe');
    frame.src = chrome.runtime.getURL('panel.html') + '#gtin=' + gtin;
    frame.title = 'Perch: product information';
    frame.setAttribute('allow', '');
    frame.setAttribute('referrerpolicy', 'no-referrer');
    const st = frame.style;
    st.setProperty('position', 'fixed', 'important');
    st.setProperty('right', '16px', 'important');
    st.setProperty('bottom', '16px', 'important');
    st.setProperty('width', 'min(360px, calc(100vw - 32px))', 'important');
    st.setProperty('height', '140px', 'important');
    st.setProperty('border', '0', 'important');
    st.setProperty('border-radius', '12px', 'important');
    st.setProperty('box-shadow', '0 4px 24px rgba(0,0,0,0.18)', 'important');
    st.setProperty('z-index', '2147483647', 'important');
    st.setProperty('color-scheme', 'normal', 'important');
    st.setProperty('background', 'transparent', 'important');
    document.documentElement.append(frame);

    window.addEventListener('message', (ev) => {
      if (ev.source !== frame.contentWindow || !ev.data || typeof ev.data !== 'object') return;
      if (ev.data.type === 'perch:height' && Number.isFinite(ev.data.height)) {
        const hgt = Math.max(60, Math.min(ev.data.height, window.innerHeight - 32));
        st.setProperty('height', `${hgt}px`, 'important');
      } else if (ev.data.type === 'perch:close') {
        frame.remove();
      }
    });
  }

  let gtin = findGtin();
  if (gtin) {
    showPanel(gtin);
    return;
  }
  // Some shops add their markup after load. Look once more.
  setTimeout(() => {
    gtin = findGtin();
    if (gtin) showPanel(gtin);
  }, 1500);
})();
