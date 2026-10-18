// Hash router. Routes look like #/product/<key>?a=b.

const table = [];

export function route(pattern, screen) {
  // pattern: '/product/:key'
  const names = [];
  const re = new RegExp(
    '^' +
      pattern.replace(/\/:([a-z]+)/g, (_, n) => {
        names.push(n);
        return '/([^/]+)';
      }) +
      '$',
  );
  table.push({ re, names, screen });
}

export function parseHash(hash = location.hash) {
  const raw = hash.replace(/^#/, '') || '/search';
  const [path, query = ''] = raw.split('?');
  const params = Object.fromEntries(new URLSearchParams(query));
  for (const r of table) {
    const m = r.re.exec(path);
    if (m) {
      const args = {};
      r.names.forEach((n, i) => {
        try {
          args[n] = decodeURIComponent(m[i + 1]);
        } catch {
          args[n] = m[i + 1];
        }
      });
      return { screen: r.screen, args, params, path };
    }
  }
  return { screen: null, args: {}, params, path };
}

export function navigate(hash, { replace = false } = {}) {
  if (replace) {
    history.replaceState(null, '', hash);
    window.dispatchEvent(new HashChangeEvent('hashchange'));
  } else {
    location.hash = hash;
  }
}
