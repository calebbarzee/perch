// Formatting for display. Views return integers in minor currency units and
// Unix milliseconds; these helpers turn them into text in the user's locale.

// Minor units come from the client's ISO 4217 table (client.currencies()),
// not from Intl, whose defaults differ for some codes (IQD, LBP, ALL, RSD,
// IRR, YER, and others).
const minorUnits = new Map();
const moneyFormats = new Map();

export function setCurrencyTable(list) {
  minorUnits.clear();
  moneyFormats.clear();
  // minor_units is null where ISO 4217 lists none (metals, testing codes);
  // those codes are not valid prices and stay unknown here.
  for (const c of list || []) if (Number.isInteger(c.minor_units)) minorUnits.set(c.code, c.minor_units);
}

export function knownCurrency(code) {
  return minorUnits.has(code);
}

export function currencyCodes() {
  return [...minorUnits.keys()].sort();
}

export function minorDigits(currency) {
  return minorUnits.get(currency) ?? 2;
}

function moneyFormat(currency) {
  if (!moneyFormats.has(currency)) {
    let fmt = null;
    const digits = minorDigits(currency);
    try {
      fmt = new Intl.NumberFormat(undefined, { style: 'currency', currency, minimumFractionDigits: digits, maximumFractionDigits: digits });
    } catch {
      fmt = null;
    }
    moneyFormats.set(currency, fmt);
  }
  return moneyFormats.get(currency);
}

export function money(amount, currency) {
  if (amount == null) return '';
  const digits = minorDigits(currency);
  const fmt = moneyFormat(currency);
  if (!fmt || !minorUnits.has(currency)) {
    // Unknown code: show the raw minor-unit amount, so nothing is guessed.
    return `${amount.toLocaleString()} ${currency} (minor units)`;
  }
  return fmt.format(amount / 10 ** digits);
}

// "129.95" in USD -> 12995. Returns null when the text is not a valid amount.
export function parseMoney(text, currency) {
  const digits = minorDigits(currency);
  const s = String(text).trim().replace(/,/g, '');
  const m = /^(\d+)(?:\.(\d+))?$/.exec(s);
  if (!m) return null;
  const frac = m[2] || '';
  if (frac.length > digits) return null;
  const value = Number(m[1] + frac.padEnd(digits, '0'));
  return Number.isSafeInteger(value) ? value : null;
}

const dateFmt = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' });
const dateTimeFmt = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });
const shortDateFmt = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' });
const monthYearFmt = new Intl.DateTimeFormat(undefined, { month: 'short', year: 'numeric' });

export const date = (ms) => (ms == null ? '' : dateFmt.format(new Date(ms)));
export const dateTime = (ms) => (ms == null ? '' : dateTimeFmt.format(new Date(ms)));
export const shortDate = (ms) => shortDateFmt.format(new Date(ms));
export const monthYear = (ms) => monthYearFmt.format(new Date(ms));
export const isoDateTime = (ms) => new Date(ms).toISOString();

export function shortHex(hex, n = 8) {
  if (!hex) return '';
  return hex.length > n ? hex.slice(0, n) : hex;
}

export function plural(n, one, many = `${one}s`) {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

export function number(n) {
  return n == null ? '' : n.toLocaleString();
}

export function mean(m) {
  return m == null ? '–' : m.toFixed(2).replace(/0$/, '').replace(/\.0$/, '');
}

export function host(url) {
  try {
    const u = new URL(url);
    return u.protocol === 'https:' ? u.hostname : null;
  } catch {
    return null;
  }
}

export const AVAILABILITY = {
  in_stock: 'In stock',
  out_of_stock: 'Out of stock',
  preorder: 'Preorder',
  discontinued: 'Discontinued',
};

export const CONDITION = { new: 'New', used: 'Used', refurbished: 'Refurbished' };

export const FLAG_REASONS = {
  fraud: 'Fraud',
  counterfeit: 'Counterfeit',
  misleading: 'Misleading',
  spam: 'Spam',
  illegal: 'Illegal',
  personal_data: 'Personal data',
  other: 'Other',
};

export const ROLES = {
  seller: 'Seller',
  buyer: 'Buyer',
  tester: 'Tester',
  crawler: 'Crawler',
  attestor: 'Attestor',
  node: 'Node operator',
  other: 'Other',
};

export function reasonsText(reasons) {
  const parts = Object.entries(reasons || {})
    .sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))
    .map(([r, n]) => `${FLAG_REASONS[r] || r} ${n}`);
  return parts.join(', ');
}

export function categoryText(cat) {
  return cat ? cat.split('/').join(' / ') : '';
}

// GTIN as printed under the barcode (docs/ux.md 8.2): from the 14-digit
// form, the last 8 digits when the first 6 are 0, else the last 12 when the
// first 2 are 0, else the last 13 when the first is 0, else all 14.
export function printedGtin(gtin14) {
  const g = String(gtin14 || '');
  if (!/^\d{14}$/.test(g)) return g;
  if (g.startsWith('000000')) return g.slice(6);
  if (g.startsWith('00')) return g.slice(2);
  if (g.startsWith('0')) return g.slice(1);
  return g;
}

// ProductKey -> label for humans.
export function keyLabel(key) {
  if (!key) return '';
  if (key.startsWith('gtin:')) return `GTIN ${printedGtin(key.slice(5))}`;
  if (key.startsWith('mpn:')) return `MPN ${key.slice(4)}`;
  if (key.startsWith('rec:')) return `Record ${shortHex(key.slice(4))}`;
  return key;
}

// Product key of a product record body (ARCHITECTURE 4.8; perch-core
// product_key.rs): gtin: + 14 digits, mpn: + brand + ":" + MPN (trimmed of
// ASCII whitespace, ASCII-lowercased, "%" and ":" escaped), else rec: + root.
export function productKeyOf(body, rootId) {
  if (body.gtin) return 'gtin:' + String(body.gtin).padStart(14, '0');
  const trim = (x) => String(x).replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, '').replace(/[A-Z]/g, (c) => c.toLowerCase());
  const esc = (x) => x.replace(/%/g, '%25').replace(/:/g, '%3a');
  if (body.mpn && body.brand) return 'mpn:' + esc(trim(body.brand)) + ':' + esc(trim(body.mpn));
  return 'rec:' + rootId;
}
