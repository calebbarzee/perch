// Canonical JSON per ARCHITECTURE 4.2 (RFC 8785 restricted to integers).
// Used by the mock client and for display. The wasm client never relies on it.
//
// JavaScript's default string sort compares UTF-16 code units, which is the
// key order RFC 8785 requires. JSON.stringify escapes strings the RFC 8785 way:
// only `"`, `\`, and controls below U+0020, with \b \t \n \f \r where defined.

export function canonicalJson(value) {
  if (value === null) return 'null';
  switch (typeof value) {
    case 'boolean':
      return value ? 'true' : 'false';
    case 'number':
      if (!Number.isSafeInteger(value) || Object.is(value, -0)) {
        throw new Error(`canonical JSON allows only safe integers, got ${value}`);
      }
      return String(value);
    case 'string':
      return JSON.stringify(value);
    case 'object': {
      if (Array.isArray(value)) return '[' + value.map(canonicalJson).join(',') + ']';
      const keys = Object.keys(value).filter((k) => value[k] !== undefined).sort();
      return '{' + keys.map((k) => JSON.stringify(k) + ':' + canonicalJson(value[k])).join(',') + '}';
    }
    default:
      throw new Error(`canonical JSON cannot encode ${typeof value}`);
  }
}

// Policy hash normalization per ARCHITECTURE 7.1: remove `now`, remove null
// fields, sort trusted_attestors by author, sort each kinds list.
export function normalizePolicy(policy) {
  const out = {};
  for (const [k, v] of Object.entries(policy || {})) {
    if (k === 'now' || v === null || v === undefined) continue;
    out[k] = v;
  }
  if (Array.isArray(out.trusted_attestors)) {
    out.trusted_attestors = out.trusted_attestors
      .map((a) => {
        const entry = {};
        for (const [k, v] of Object.entries(a)) if (v !== null && v !== undefined) entry[k] = v;
        if (Array.isArray(entry.kinds)) entry.kinds = [...entry.kinds].sort();
        return entry;
      })
      .sort((x, y) => (x.author < y.author ? -1 : x.author > y.author ? 1 : 0));
  }
  return out;
}
