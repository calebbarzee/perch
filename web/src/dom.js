// DOM helpers. Text is always inserted with text nodes, never parsed as HTML
// (ARCHITECTURE 10). Text that comes from records goes through userText(),
// which isolates it for bidirectional rendering with dir="auto".

const SVG_NS = 'http://www.w3.org/2000/svg';

// h('div', { class: 'x', on: { click } }, 'text', child, [more])
// A tag prefixed with "svg:" is created in the SVG namespace.
export function h(tag, props, ...children) {
  let el;
  if (tag.startsWith('svg:')) el = document.createElementNS(SVG_NS, tag.slice(4));
  else el = document.createElement(tag);
  if (props) {
    for (const [key, value] of Object.entries(props)) {
      if (value === undefined || value === null || value === false) continue;
      if (key === 'on') {
        for (const [ev, fn] of Object.entries(value)) el.addEventListener(ev, fn);
      } else if (key === 'text') {
        el.textContent = String(value);
      } else if (key === 'class') {
        el.setAttribute('class', Array.isArray(value) ? value.filter(Boolean).join(' ') : value);
      } else if (key === 'dataset') {
        for (const [dk, dv] of Object.entries(value)) el.dataset[dk] = dv;
      } else if (key === 'value' && 'value' in el) {
        el.value = value;
      } else if (key === 'checked' || key === 'selected' || key === 'disabled') {
        el[key] = Boolean(value);
      } else if (key === 'href') {
        el.setAttribute('href', safeHref(value));
      } else {
        el.setAttribute(key, value === true ? '' : String(value));
      }
    }
  }
  append(el, children);
  return el;
}

export function append(el, children) {
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

// Record text: textContent plus dir="auto".
export function userText(tag, text, props = {}) {
  const el = h(tag, { ...props, dir: 'auto' });
  el.textContent = text == null ? '' : String(text);
  return el;
}

// Inline record text inside a sentence: <bdi>.
export function bdi(text, props = {}) {
  return userText('bdi', text, props);
}

// Only internal hash links, https URLs, and blob/data downloads are allowed.
export function safeHref(value) {
  const s = String(value);
  if (s.startsWith('#') || s.startsWith('blob:')) return s;
  try {
    const u = new URL(s, location.href);
    if (u.protocol === 'https:' || u.protocol === 'http:') {
      // http only for same-origin app paths (node API links).
      if (u.protocol === 'https:' || u.origin === location.origin) return u.href;
    }
  } catch {
    // fall through
  }
  return '#';
}

export function clear(el) {
  while (el.firstChild) el.removeChild(el.firstChild);
  return el;
}

// A link to a URL taken from a record. Shows the ASCII host so a lookalike
// Unicode host cannot pose as another (ARCHITECTURE 4.5).
export function externalLink(url, label) {
  let host = null;
  try {
    const u = new URL(url);
    if (u.protocol === 'https:') host = u.hostname;
  } catch {
    host = null;
  }
  if (!host) return h('span', { class: 'bad-url' }, 'Invalid URL');
  return h(
    'a',
    {
      href: url,
      rel: 'noopener noreferrer nofollow external',
      referrerpolicy: 'no-referrer',
      target: '_blank',
      class: 'external',
    },
    label ? [bdi(label), ' '] : null,
    h('span', { class: 'host', text: host }),
    h('span', { class: 'visually-hidden', text: ' (opens in a new tab)' }),
  );
}

export function announce(message) {
  const live = document.getElementById('live');
  if (!live) return;
  live.textContent = '';
  // A separate task so screen readers notice repeated messages.
  setTimeout(() => {
    live.textContent = message;
  }, 30);
}

export function field({ id, label, hint, control }) {
  if (!control.id) control.id = id;
  const hintId = hint ? `${control.id}-hint` : null;
  if (hintId) control.setAttribute('aria-describedby', hintId);
  return h(
    'div',
    { class: 'field' },
    h('label', { for: control.id, text: label }),
    hint ? h('p', { class: 'hint', id: hintId, text: hint }) : null,
    control,
  );
}

export function downloadText(filename, text, type = 'application/json') {
  const blob = new Blob([text], { type });
  const url = URL.createObjectURL(blob);
  const a = h('a', { href: url, download: filename });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    announce('Copied');
    return true;
  } catch {
    announce('Copy failed. Select the text and copy it manually.');
    return false;
  }
}
