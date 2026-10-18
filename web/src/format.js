// Formatting for display. Views return integers in minor currency units and
// Unix milliseconds; these helpers turn them into text in the user's locale.

const moneyFormats = new Map();

function moneyFormat(currency) {
  if (!moneyFormats.has(currency)) {
    let fmt = null;
    try {
      fmt = new Intl.NumberFormat(undefined, { style: 'currency', currency });
    } catch {
      fmt = null;
    }
    moneyFormats.set(currency, fmt);
  }
  return moneyFormats.get(currency);
}

export function minorDigits(currency) {
  const fmt = moneyFormat(currency);
  return fmt ? fmt.resolvedOptions().maximumFractionDigits : 2;
}

export function money(amount, currency) {
  if (amount == null) return '';
  const fmt = moneyFormat(currency);
  if (!fmt) return `${amount} ${currency} (minor units)`;
  return fmt.format(amount / 10 ** minorDigits(currency));
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

// ProductKey -> label for humans.
export function keyLabel(key) {
  if (!key) return '';
  if (key.startsWith('gtin:')) return `GTIN ${key.slice(5)}`;
  if (key.startsWith('mpn:')) return `MPN ${key.slice(4)}`;
  if (key.startsWith('rec:')) return `Record ${shortHex(key.slice(4))}`;
  return key;
}
