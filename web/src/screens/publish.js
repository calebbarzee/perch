import { h, bdi, field, announce, copyText, downloadText, userText } from '../dom.js';
import * as fmt from '../format.js';
import { notice, routes, authorChip, recordLink, hex, priceText } from '../components.js';
import { backupConfirmed, publishBlocker } from './keys.js';
import { persistKey } from '../app.js';
import { canonicalJson } from '../canonical.js';

const KINDS = [
  ['review', 'Review'],
  ['listing', 'Listing'],
  ['receipt', 'Receipt for a buyer'],
  ['product', 'Product'],
  ['profile', 'Profile'],
  ['flag', 'Flag'],
  ['appeal', 'Appeal'],
  ['retract', 'Withdraw a record'],
];

const COMMON = ['EUR', 'USD', 'GBP', 'CHF', 'SEK', 'NOK', 'DKK', 'PLN', 'CZK', 'JPY', 'CAD', 'AUD'];

export default {
  id: 'publish',
  live: (ctx) => JSON.stringify([ctx.client.author(), publishBlocker(ctx.client), ctx.client.peers().map((p) => p.state)]),
  render(ctx) {
    const { client, params } = ctx;
    const kind = KINDS.some(([k]) => k === params.kind) ? params.kind : 'review';
    const section = h('section', { class: 'screen screen-publish' }, h('h1', { text: 'Publish' }));
    section.append(h('p', { class: 'lede', text: 'Each record is signed with your key on this device, then sent to the connected nodes. Records are public and permanent: you can replace or withdraw them, but the earlier versions stay in the data.' }));

    const tabs = h('nav', { class: 'kind-tabs', 'aria-label': 'Record type' },
      h('ul', {}, KINDS.map(([k, label]) => h('li', {}, h('a', { href: `#/publish?kind=${k}`, 'aria-current': k === kind ? 'page' : null, text: label })))),
    );
    section.append(tabs);

    const author = client.author();
    if (!author) {
      section.append(notice('info', 'You need a key to publish', h('p', {}, 'Create or import a key first. ', h('a', { href: '#/keys', text: 'Go to Keys' }), '.')));
      return section;
    }
    section.append(h('p', { class: 'signing-as' }, 'Signing as ', authorChip(author, client.authorView(author)?.name ?? null)));

    const blockedReasons = [];
    if (!backupConfirmed(client)) blockedReasons.push(h('p', {}, 'Confirm that you saved a backup of your key. ', h('a', { href: '#/keys', text: 'Go to Keys' }), '.'));
    const guard = publishBlocker(client);
    if (guard) blockedReasons.push(h('p', { text: guard }));
    const blocked = blockedReasons.length > 0;
    if (blocked && kind !== 'receipt') section.append(notice('warn', 'Publishing is paused', ...blockedReasons));

    const forms = { review: reviewForm, listing: listingForm, receipt: receiptTool, product: productForm, profile: profileForm, flag: flagForm, appeal: appealForm, retract: retractForm };
    section.append(forms[kind](ctx, { blocked: kind === 'receipt' ? false : blocked }));
    return section;
  },
};

// Shared submit flow: build body, show preview, publish, report.
function publishForm(ctx, { id, kind, fields, build, blocked, submitLabel = 'Sign and publish', replaces, afterPublish }) {
  const { client } = ctx;
  const status = h('div', { class: 'form-status', role: 'status', 'aria-live': 'polite' });
  const preview = h('details', { class: 'record-preview' }, h('summary', { text: 'Preview the record body' }), h('pre', { class: 'json' }));
  const form = h('form', { class: 'publish-form panel', id, novalidate: true }, ...fields, preview,
    h('p', { class: 'actions' }, h('button', { type: 'submit', class: 'button primary', disabled: blocked, text: submitLabel })),
    status,
  );
  const updatePreview = () => {
    try {
      const body = build(form);
      preview.querySelector('pre').textContent = JSON.stringify(body, null, 2);
    } catch (e) {
      preview.querySelector('pre').textContent = e.message;
    }
  };
  form.addEventListener('input', updatePreview);
  form.addEventListener('change', updatePreview);
  requestAnimationFrame(updatePreview);
  form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    if (blocked) return;
    let body;
    try {
      body = build(form, { strict: true });
    } catch (e) {
      status.replaceChildren(notice('bad', 'Check the form', h('p', { text: e.message })));
      const bad = e.field && form.elements[e.field];
      bad?.focus?.();
      return;
    }
    try {
      const opts = {};
      const rep = typeof replaces === 'function' ? replaces(form) : replaces;
      if (rep) opts.replaces = rep;
      const res = client.publish(kind, body, opts);
      await persistKey();
      announce('Published.');
      status.replaceChildren(
        notice('good', 'Published',
          h('p', {}, 'Record ', recordLink(res.id), ' is signed and stored on this device. Connected nodes receive it now.'),
          afterPublish ? afterPublish(res, body) : null,
        ),
      );
      form.querySelector('button[type=submit]').disabled = true;
    } catch (e) {
      status.replaceChildren(notice('bad', 'Not published', h('p', { text: e.message || String(e) })));
    }
  });
  return form;
}

function fail(message, fieldName) {
  const e = new Error(message);
  e.field = fieldName;
  throw e;
}

function text(form, name) {
  return (form.elements[name]?.value ?? '').trim();
}

function productPicker(ctx, name, value, label = 'Product') {
  const list = h('datalist', { id: `${name}-options` });
  for (const hit of ctx.client.search('').slice(0, 200)) list.append(h('option', { value: hit.key, text: `${hit.name} (${hit.brand})` }));
  const input = h('input', { name, id: `f-${name}`, value: value || '', list: `${name}-options`, autocomplete: 'off', spellcheck: 'false', class: 'mono', required: true });
  const shown = h('p', { class: 'hint picked' });
  const update = () => {
    const p = ctx.client.product(input.value.trim());
    shown.replaceChildren(p ? h('span', {}, 'Selected: ', bdi(p.name?.value ?? p.key)) : input.value ? 'No product with this key in your local data.' : '');
  };
  input.addEventListener('input', update);
  requestAnimationFrame(update);
  return h('div', {}, field({ id: `f-${name}`, label, hint: 'Product key, for example gtin:04006381333931. Type to choose from your local data.', control: input }), list, shown);
}

function reviewForm(ctx, { blocked }) {
  const { client, params } = ctx;
  const receiptInput = h('textarea', { name: 'receipt', id: 'f-receipt', rows: 4, class: 'mono', spellcheck: 'false' });
  const receiptStatus = h('div', { class: 'receipt-status', role: 'status', 'aria-live': 'polite' });
  const checkReceipt = () => {
    const raw = receiptInput.value.trim();
    if (!raw) {
      receiptStatus.replaceChildren();
      return null;
    }
    let parsed;
    try {
      parsed = JSON.parse(raw);
    } catch {
      receiptStatus.replaceChildren(notice('bad', 'Not a receipt', h('p', { text: 'The receipt is not valid JSON. Paste it exactly as the seller sent it.' })));
      return null;
    }
    const res = client.verifyReceipt(parsed);
    const me = client.author();
    const parts = [];
    if (res.valid) {
      const l = client.listing(parsed.listing);
      parts.push(h('p', {}, 'Signed by the seller of listing ', l ? h('a', { href: routes.listing(l.id) }, bdi(l.title || l.seller_name || fmt.shortHex(l.id))) : hex(parsed.listing), '. Seller: ', authorChip(res.seller, l?.seller_name ?? null, { verified: l?.seller_verified })));
      if (parsed.buyer !== me) parts.push(h('p', { class: 'bad-text', text: 'This receipt names a different buyer key. It will not count as a verified purchase for you.' }));
      const productKey = text(receiptInput.form, 'product');
      if (l && productKey && l.product !== productKey) parts.push(h('p', { class: 'bad-text', text: 'This receipt is for a different product.' }));
      if (l && !l.seller_verified) parts.push(h('p', { class: 'warn-text', text: "The seller's identity is not verified under your policy, so the review will not count as a verified purchase." }));
      parts.push(h('p', { class: 'hint', text: `Purchased ${fmt.dateTime(parsed.purchased_at)}. Publishing makes this receipt public.` }));
      receiptStatus.replaceChildren(notice(parsed.buyer === me ? 'good' : 'warn', 'Receipt verified', ...parts));
    } else {
      receiptStatus.replaceChildren(notice('bad', 'Receipt does not verify', h('p', { text: res.error || 'Unknown error.' })));
    }
    return parsed;
  };
  receiptInput.addEventListener('input', checkReceipt);

  const rating = h('fieldset', { class: 'rating-input' },
    h('legend', { text: 'Rating' }),
    ...[1, 2, 3, 4, 5].map((n) =>
      h('label', { class: 'rating-option' }, h('input', { type: 'radio', name: 'rating', value: String(n), required: true }), h('span', { text: `${n}` }), h('span', { class: 'visually-hidden', text: n === 1 ? ' star' : ' stars' })),
    ),
  );

  return publishForm(ctx, {
    id: 'form-review',
    kind: 'review',
    blocked,
    replaces: params.replaces || null,
    fields: [
      productPicker(ctx, 'product', params.product),
      rating,
      field({ id: 'f-title', label: 'Title (optional)', control: h('input', { name: 'title', id: 'f-title', maxlength: 200, dir: 'auto' }) }),
      field({ id: 'f-text', label: 'Review', hint: 'What you observed. Up to 10,000 characters.', control: h('textarea', { name: 'text', id: 'f-text', rows: 6, maxlength: 10000, required: true, dir: 'auto' }) }),
      field({ id: 'f-receipt', label: 'Receipt from the seller (optional)', hint: 'Paste the receipt JSON the seller gave you. It is checked here before you publish.', control: receiptInput }),
      receiptStatus,
    ],
    build(form, { strict } = {}) {
      const body = { product: text(form, 'product') };
      const r = form.querySelector('input[name=rating]:checked');
      if (strict && !body.product) fail('Choose a product.', 'product');
      if (strict && !r) fail('Choose a rating from 1 to 5.');
      body.rating = r ? Number(r.value) : 0;
      body.text = text(form, 'text');
      if (strict && !body.text) fail('Write the review text.', 'text');
      const title = text(form, 'title');
      if (title) body.title = title;
      const raw = text(form, 'receipt');
      if (raw) {
        let rc;
        try {
          rc = JSON.parse(raw);
        } catch {
          fail('The receipt is not valid JSON.', 'receipt');
        }
        if (strict) {
          const v = client.verifyReceipt(rc);
          if (!v.valid) fail(`The receipt does not verify: ${v.error}`, 'receipt');
        }
        body.receipt = rc;
        body.listing = rc.listing;
      }
      return body;
    },
    afterPublish: (res, body) => h('p', {}, h('a', { href: routes.product(body.product), text: 'View the product' })),
  });
}

function priceFields(prefix = '') {
  const all = fmt.currencyCodes();
  const common = COMMON.filter((c) => fmt.knownCurrency(c));
  const cur = h('select', { name: 'currency', id: `f-${prefix}currency` },
    h('optgroup', { label: 'Common' }, common.map((c) => h('option', { value: c, text: c }))),
    h('optgroup', { label: 'All ISO 4217 codes' }, all.filter((c) => !common.includes(c)).map((c) => h('option', { value: c, text: c }))),
  );
  return [
    h('div', { class: 'row' },
      field({ id: `f-${prefix}amount`, label: 'Price', hint: 'For example 59.99', control: h('input', { name: 'amount', id: `f-${prefix}amount`, inputmode: 'decimal', required: true, autocomplete: 'off' }) }),
      field({ id: `f-${prefix}currency`, label: 'Currency', control: cur }),
    ),
  ];
}

function listingForm(ctx, { blocked }) {
  const { client, params } = ctx;
  const prev = params.replaces ? client.record(params.replaces) : null;
  const prevBody = prev?.body || {};
  const avail = h('select', { name: 'availability', id: 'f-availability' }, Object.entries(fmt.AVAILABILITY).map(([v, l]) => h('option', { value: v, selected: prevBody.availability === v, text: l })));
  const cond = h('select', { name: 'condition', id: 'f-condition' }, Object.entries(fmt.CONDITION).map(([v, l]) => h('option', { value: v, selected: prevBody.condition === v, text: l })));
  const form = publishForm(ctx, {
    id: 'form-listing',
    kind: 'listing',
    blocked,
    replaces: params.replaces || null,
    fields: [
      prev ? notice('info', 'Updating your listing', h('p', {}, 'This publishes a new version of ', recordLink(params.replaces), '. The product cannot change.')) : null,
      productPicker(ctx, 'product', prevBody.product || params.product),
      field({ id: 'f-url', label: 'Product page URL', hint: 'https only. Prices count in ranges only when an attestor verified your domain.', control: h('input', { name: 'url', id: 'f-url', type: 'url', value: prevBody.url || '', required: true, spellcheck: 'false' }) }),
      field({ id: 'f-ltitle', label: 'Title (optional)', control: h('input', { name: 'title', id: 'f-ltitle', maxlength: 300, dir: 'auto', value: prevBody.title || '' }) }),
      ...priceFields(),
      h('div', { class: 'row' }, field({ id: 'f-availability', label: 'Availability', control: avail }), field({ id: 'f-condition', label: 'Condition', control: cond })),
      field({ id: 'f-ships', label: 'Ships to (optional)', hint: 'Two-letter country codes separated by commas, for example DE, AT, NL', control: h('input', { name: 'ships_to', id: 'f-ships', value: (prevBody.ships_to || []).join(', '), spellcheck: 'false' }) }),
    ],
    build(form, { strict } = {}) {
      const currency = form.elements.currency.value;
      const amount = fmt.parseMoney(text(form, 'amount'), currency);
      const url = text(form, 'url');
      const body = { product: text(form, 'product'), url, price: { amount: amount ?? 0, currency }, availability: form.elements.availability.value, condition: form.elements.condition.value };
      if (strict) {
        if (!body.product) fail('Choose a product.', 'product');
        if (amount == null) fail(`Enter the price as a number with at most ${fmt.minorDigits(currency)} decimals.`, 'amount');
        if (!fmt.host(url)) fail('The URL must start with https:// and name a domain.', 'url');
      }
      const title = text(form, 'title');
      if (title) body.title = title;
      const ships = text(form, 'ships_to').split(/[\s,]+/).filter(Boolean).map((s) => s.toUpperCase());
      if (ships.length) {
        if (strict && ships.some((s) => !/^[A-Z]{2}$/.test(s))) fail('Use two-letter country codes for "Ships to".', 'ships_to');
        body.ships_to = ships;
      }
      return body;
    },
    afterPublish: (res) => h('p', {}, h('a', { href: routes.listing(params.root || res.id), text: 'View the listing' })),
  });
  if (prevBody.price) {
    requestAnimationFrame(() => {
      form.elements.currency.value = prevBody.price.currency;
      form.elements.amount.value = (prevBody.price.amount / 10 ** fmt.minorDigits(prevBody.price.currency)).toFixed(fmt.minorDigits(prevBody.price.currency));
    });
  }
  return form;
}

function receiptTool(ctx) {
  const { client, params } = ctx;
  const me = client.author();
  const mine = [];
  for (const hit of client.search('')) {
    const p = client.product(hit.key);
    for (const l of p?.listings || []) if (l.seller === me) mine.push({ l, name: p.name?.value ?? p.key });
  }
  const out = h('div', { class: 'receipt-out' });
  const listingSel = h('select', { name: 'listing', id: 'f-rlisting' },
    mine.length ? mine.map(({ l, name }) => h('option', { value: l.id, selected: l.id === params.listing, text: `${name}, ${priceText(l.price)}` })) : h('option', { value: '', text: 'You have no listings' }),
  );
  const buyer = h('input', { name: 'buyer', id: 'f-buyer', class: 'mono', spellcheck: 'false', autocomplete: 'off', required: true });
  const when = h('input', { name: 'purchased', id: 'f-purchased', type: 'datetime-local', required: true });
  const d = new Date();
  d.setSeconds(0, 0);
  when.value = new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
  const form = h('form', { class: 'panel', id: 'form-receipt', novalidate: true },
    h('h2', { text: 'Issue a receipt' }),
    h('p', { text: 'A receipt lets a buyer mark their review as a verified purchase. It is not published. Send it to the buyer privately; it becomes public only if they cite it in a review. You cannot revoke it.' }),
    field({ id: 'f-rlisting', label: 'Your listing', control: listingSel }),
    field({ id: 'f-buyer', label: "Buyer's public key", hint: '64 hex characters. The buyer finds it on their Keys screen.', control: buyer }),
    field({ id: 'f-purchased', label: 'Purchase time', control: when }),
    h('p', { class: 'actions' }, h('button', { type: 'submit', class: 'button primary', disabled: !mine.length, text: 'Sign receipt' })),
    out,
  );
  form.addEventListener('submit', (ev) => {
    ev.preventDefault();
    try {
      const b = buyer.value.trim().toLowerCase();
      if (!/^[0-9a-f]{64}$/.test(b)) fail("The buyer's key must be 64 hex characters.", 'buyer');
      const ts = new Date(when.value).getTime();
      if (!Number.isSafeInteger(ts)) fail('Enter the purchase time.', 'purchased');
      const receipt = client.issueReceipt(listingSel.value, b, ts);
      const json = canonicalJson(receipt);
      out.replaceChildren(
        notice('good', 'Receipt signed',
          h('p', { text: 'Send this text to the buyer. Anyone who has it can see the listing, the buyer key, and the purchase time.' }),
          h('pre', { class: 'json receipt-json', id: 'receipt-json', tabindex: '0', text: json }),
          h('p', { class: 'actions' },
            h('button', { type: 'button', class: 'button', on: { click: () => copyText(json) } }, 'Copy receipt'),
            ' ',
            h('button', { type: 'button', class: 'button', on: { click: () => downloadText(`perch-receipt-${receipt.nonce.slice(0, 8)}.json`, json + '\n') } }, 'Download receipt'),
          ),
        ),
      );
      announce('Receipt signed.');
    } catch (e) {
      out.replaceChildren(notice('bad', 'Receipt not signed', h('p', { text: e.message })));
    }
  });
  return form;
}

function productForm(ctx, { blocked }) {
  return publishForm(ctx, {
    id: 'form-product',
    kind: 'product',
    blocked,
    fields: [
      notice('info', 'Before you add a product', h('p', { text: 'Search first. Records with the same GTIN, or the same brand and MPN, describe the same product and merge automatically.' })),
      field({ id: 'f-pname', label: 'Name', control: h('input', { name: 'name', id: 'f-pname', maxlength: 300, required: true, dir: 'auto' }) }),
      field({ id: 'f-brand', label: 'Brand', control: h('input', { name: 'brand', id: 'f-brand', maxlength: 100, required: true, dir: 'auto' }) }),
      h('div', { class: 'row' },
        field({ id: 'f-gtin', label: 'GTIN (optional)', hint: 'The barcode number: 8, 12, 13, or 14 digits', control: h('input', { name: 'gtin', id: 'f-gtin', inputmode: 'numeric', autocomplete: 'off' }) }),
        field({ id: 'f-mpn', label: 'MPN (optional)', hint: 'Manufacturer part number', control: h('input', { name: 'mpn', id: 'f-mpn', maxlength: 100 }) }),
      ),
      field({ id: 'f-cat', label: 'Category (optional)', hint: 'Lowercase, separated by /, for example appliances/kitchen', control: h('input', { name: 'category', id: 'f-cat', maxlength: 200 }) }),
      field({ id: 'f-desc', label: 'Description (optional)', control: h('textarea', { name: 'description', id: 'f-desc', rows: 4, maxlength: 5000, dir: 'auto' }) }),
    ],
    build(form, { strict } = {}) {
      const body = { name: text(form, 'name'), brand: text(form, 'brand') };
      if (strict && !body.name) fail('Enter the product name.', 'name');
      if (strict && !body.brand) fail('Enter the brand.', 'brand');
      const gtin = text(form, 'gtin');
      if (gtin) {
        if (strict && !validGtin(gtin)) fail('The GTIN must have 8, 12, 13, or 14 digits and a valid check digit.', 'gtin');
        body.gtin = gtin;
      }
      for (const k of ['mpn', 'category', 'description']) {
        const v = text(form, k);
        if (v) body[k] = k === 'category' ? v.toLowerCase() : v;
      }
      return body;
    },
  });
}

export function validGtin(g) {
  if (!/^(\d{8}|\d{12,14})$/.test(g)) return false;
  const digits = g.split('').map(Number);
  const check = digits.pop();
  let sum = 0;
  digits.reverse().forEach((d, i) => {
    sum += d * (i % 2 === 0 ? 3 : 1);
  });
  return (10 - (sum % 10)) % 10 === check;
}

function profileForm(ctx, { blocked }) {
  const { client } = ctx;
  const me = client.authorView(client.author());
  const role = h('select', { name: 'role', id: 'f-role' }, Object.entries(fmt.ROLES).map(([v, l]) => h('option', { value: v, selected: me?.role === v, text: l })));
  return publishForm(ctx, {
    id: 'form-profile',
    kind: 'profile',
    blocked,
    fields: [
      field({ id: 'f-name', label: 'Display name', hint: 'Anyone can choose any name. People recognize you by your key.', control: h('input', { name: 'name', id: 'f-name', maxlength: 100, required: true, dir: 'auto', value: me?.name || '' }) }),
      field({ id: 'f-role', label: 'Role', control: role }),
      field({ id: 'f-purl', label: 'Website (optional)', control: h('input', { name: 'url', id: 'f-purl', type: 'url', value: me?.url || '', spellcheck: 'false' }) }),
      field({ id: 'f-about', label: 'About (optional)', control: h('textarea', { name: 'about', id: 'f-about', rows: 3, maxlength: 2000, dir: 'auto', text: me?.about || '' }) }),
    ],
    build(form, { strict } = {}) {
      const body = { name: text(form, 'name'), role: form.elements.role.value };
      if (strict && !body.name) fail('Enter a display name.', 'name');
      const url = text(form, 'url');
      if (url) {
        if (strict && !fmt.host(url)) fail('The website must start with https://.', 'url');
        body.url = url;
      }
      const about = text(form, 'about');
      if (about) body.about = about;
      return body;
    },
  });
}

function targetField(ctx, value, label = 'Record ID') {
  const input = h('input', { name: 'target', id: 'f-target', class: 'mono', value: value || '', spellcheck: 'false', autocomplete: 'off', required: true });
  const shown = h('div', { class: 'hint picked' });
  const update = () => {
    const id = input.value.trim();
    const rec = /^[0-9a-f]{64}$/.test(id) ? ctx.client.record(id) : null;
    shown.replaceChildren(rec ? h('span', {}, h('code', { text: rec.kind }), ' by ', authorChip(rec.author, ctx.client.authorView(rec.author)?.name ?? null, { link: false }), rec.body?.text ? [': ', userText('q', rec.body.text.slice(0, 120))] : rec.body?.title ? [': ', bdi(rec.body.title)] : null) : id ? 'No record with this ID in your local data.' : '');
  };
  input.addEventListener('input', update);
  requestAnimationFrame(update);
  return h('div', {}, field({ id: 'f-target', label, hint: '64 hex characters', control: input }), shown);
}

function flagForm(ctx, { blocked }) {
  const reason = h('select', { name: 'reason', id: 'f-reason' }, Object.entries(fmt.FLAG_REASONS).map(([v, l]) => h('option', { value: v, text: l })));
  return publishForm(ctx, {
    id: 'form-flag',
    kind: 'flag',
    blocked,
    fields: [
      h('p', { text: 'A flag is a public, signed report. Listings, reviews, products, and profiles are hidden when enough identity-verified people flag them. You can withdraw your flag later.' }),
      targetField(ctx, ctx.params.target),
      field({ id: 'f-reason', label: 'Reason', control: reason }),
      field({ id: 'f-note', label: 'Note (optional)', hint: 'What you observed. Public.', control: h('textarea', { name: 'note', id: 'f-note', rows: 3, maxlength: 2000, dir: 'auto' }) }),
    ],
    build(form, { strict } = {}) {
      const body = { target: text(form, 'target'), reason: form.elements.reason.value };
      if (strict && !/^[0-9a-f]{64}$/.test(body.target)) fail('The record ID must be 64 lowercase hex characters.', 'target');
      const note = text(form, 'note');
      if (note) body.note = note;
      return body;
    },
  });
}

function appealForm(ctx, { blocked }) {
  return publishForm(ctx, {
    id: 'form-appeal',
    kind: 'appeal',
    blocked,
    fields: [
      h('p', { text: 'An appeal is a public statement attached to your own flagged record. It does not change the status by itself; flaggers may read it and withdraw their flags.' }),
      targetField(ctx, ctx.params.target, 'Your record ID'),
      field({ id: 'f-anote', label: 'Statement', control: h('textarea', { name: 'note', id: 'f-anote', rows: 4, maxlength: 2000, required: true, dir: 'auto' }) }),
    ],
    build(form, { strict } = {}) {
      const body = { target: text(form, 'target'), note: text(form, 'note') };
      if (strict && !/^[0-9a-f]{64}$/.test(body.target)) fail('The record ID must be 64 lowercase hex characters.', 'target');
      if (strict && !body.note) fail('Write a statement.', 'note');
      return body;
    },
  });
}

function retractForm(ctx, { blocked }) {
  return publishForm(ctx, {
    id: 'form-retract',
    kind: 'retract',
    blocked,
    submitLabel: 'Sign and withdraw',
    fields: [
      notice('warn', 'Withdrawal is final', h('p', { text: 'Withdrawing a review, listing, flag, or other record removes it from views. The record and the withdrawal stay in the data. You cannot undo it.' })),
      targetField(ctx, ctx.params.target, 'Your record ID'),
    ],
    build(form, { strict } = {}) {
      const body = { target: text(form, 'target') };
      if (strict && !/^[0-9a-f]{64}$/.test(body.target)) fail('The record ID must be 64 lowercase hex characters.', 'target');
      if (strict) {
        const rec = ctx.client.record(body.target);
        if (rec && rec.author !== ctx.client.author()) fail('You can only withdraw your own records.', 'target');
      }
      return body;
    },
  });
}
