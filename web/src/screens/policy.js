import { h, field, announce } from '../dom.js';
import * as fmt from '../format.js';
import { notice, hex, authorChip } from '../components.js';
import { savePolicy, resetPolicy, renderFooter } from '../app.js';
import { activeNodes, saveSettings } from '../settings.js';
import { httpBaseFor } from '../client.js';
import { canonicalJson, normalizePolicy } from '../canonical.js';
import { sha256Hex } from '../sha256.js';

const NUMBER_FIELDS = [
  ['flag_threshold', 'Flags to hide a record', 'Distinct people whose flags hide a listing or profile. Reviews and products have their own thresholds below.'],
  ['seller_hidden_listing_limit', 'Hidden listings to restrict a seller', 'A seller is restricted when this many of their listings are hidden...'],
  ['seller_min_distinct_flaggers', 'Distinct flaggers to restrict a seller', '...and at least this many distinct people flagged those listings.'],
];

// Shown in minutes, stored in milliseconds.
const CLOCK = ['future_ts_tolerance_ms', 'Allowed clock difference (minutes)', 'Records dated further in the future than this are treated as not existing yet.'];

const BOOL_FIELDS = [
  ['flags_require_verified', 'Count flags from identity-verified people only'],
  ['verified_purchase_requires_verified_buyer', 'Verified purchase requires an identity-verified buyer'],
  ['price_requires_domain_verified', 'Price ranges use listings on verified seller domains only'],
];

export const hashOf = (policy) => sha256Hex(canonicalJson(normalizePolicy(policy)));

// Largest integer that JSON numbers carry exactly (2^53 − 1).
const MAX_INT = Number.MAX_SAFE_INTEGER;

// Every number in a policy must be a whole number from 0 to 2^53 − 1;
// larger ones lose precision and change the policy hash. Returns the first
// problem as text, or null.
export function policyNumberProblem(policy, path = 'policy') {
  if (typeof policy === 'number') {
    return Number.isSafeInteger(policy) && policy >= 0 ? null : `${path} must be a whole number from 0 to ${MAX_INT}.`;
  }
  if (policy && typeof policy === 'object') {
    for (const [k, v] of Object.entries(policy)) {
      const p = policyNumberProblem(v, Array.isArray(policy) ? `${path}[${k}]` : `${path}.${k}`);
      if (p) return p;
    }
  }
  return null;
}

// A whole number typed into a field, from `min` to 2^53 − 1.
function wholeNumber(raw, label, min = 0) {
  const t = String(raw ?? '').trim();
  const n = Number(t);
  if (!/^\d+$/.test(t) || !Number.isSafeInteger(n) || n < min) {
    throw new Error(`${label} must be a whole number from ${min} to ${MAX_INT}.`);
  }
  return n;
}

export default {
  id: 'policy',
  render(ctx) {
    const { client, app } = ctx;
    const policy = client.policy();
    const hash = client.policyHash();
    const bundledHash = hashOf(app.bundledPolicy);
    const isBundled = hash === bundledHash;
    const section = h('section', { class: 'screen screen-policy' }, h('h1', { text: 'Policy' }));
    section.append(
      h('p', { class: 'lede', text: 'The policy sets whose attestations you trust and when moderation hides records. Every rating, price range, and hidden notice on this device follows it. Two people with the same records and the same policy see the same results.' }),
      h('div', { class: 'panel policy-hash' },
        h('p', { class: 'label', text: 'Active policy hash' }),
        h('code', { class: 'hex hex-full', id: 'policy-hash', text: hash }),
        h('p', { class: isBundled ? 'good-text' : 'warn-text', text: isBundled ? 'This is the policy bundled with this app release.' : 'You changed the policy. It differs from the one bundled with this release.' }),
        app.mode === 'mock'
          ? h('p', { class: 'hint', text: 'The mock client uses its own demo policy.' })
          : h('p', { class: 'hint' }, 'The bundled policy is the file ', h('a', { href: 'default-policy.json', target: '_blank', rel: 'noopener' }, h('code', { text: 'default-policy.json' })), ' served with this app.'),
      ),
    );

    // Time.
    const pinned = policy.now != null;
    const nowInput = h('input', { type: 'datetime-local', id: 'p-now', step: 1, disabled: !pinned });
    const nowValue = pinned ? policy.now : Date.now();
    nowInput.value = new Date(nowValue - new Date(nowValue).getTimezoneOffset() * 60000).toISOString().slice(0, 19);
    const pin = h('input', { type: 'checkbox', id: 'p-pin', checked: pinned });
    pin.addEventListener('change', () => {
      nowInput.disabled = !pin.checked;
    });

    const form = h('form', { class: 'panel', id: 'policy-form', novalidate: true });
    form.append(h('h2', { text: 'Rules' }));
    for (const [k, label, hint] of NUMBER_FIELDS) {
      form.append(field({ id: `p-${k}`, label, hint, control: h('input', { type: 'number', id: `p-${k}`, name: k, min: 0, step: 1, value: String(policy[k] ?? 0), inputmode: 'numeric' }) }));
    }
    {
      const ms = policy[CLOCK[0]] ?? 0;
      const shown = ms % 60000 === 0 ? String(ms / 60000) : String(ms / 60000);
      const input = h('input', { type: 'number', id: 'p-clock', name: 'clock_minutes', min: 0, step: 'any', value: shown, inputmode: 'decimal' });
      input.dataset.original = shown;
      form.append(field({ id: 'p-clock', label: CLOCK[1], hint: CLOCK[2], control: input }));
    }
    form.append(field({ id: 'p-review-threshold', label: 'Flags to hide a review', hint: 'Hidden reviews still count in ratings; hiding collapses their text only.', control: h('input', { type: 'number', id: 'p-review-threshold', name: 'review_threshold', min: 1, step: 1, value: String(policy.flag_thresholds?.review ?? policy.flag_threshold ?? 5) }) }));
    form.append(field({ id: 'p-product-threshold', label: 'Flags to hide a product record', control: h('input', { type: 'number', id: 'p-product-threshold', name: 'product_threshold', min: 1, step: 1, value: String(policy.flag_thresholds?.product ?? policy.flag_threshold ?? 5) }) }));
    for (const [k, label] of BOOL_FIELDS) {
      form.append(h('div', { class: 'check' }, h('input', { type: 'checkbox', id: `p-${k}`, name: k, checked: Boolean(policy[k]) }), h('label', { for: `p-${k}`, text: label })));
    }
    form.append(
      h('fieldset', {},
        h('legend', { text: 'Time' }),
        h('div', { class: 'check' }, pin, h('label', { for: 'p-pin', text: 'Pin the time used by views' })),
        field({ id: 'p-now', label: 'Pinned time', hint: 'Unpinned, views use the current time at each query. Pin it to reproduce results from another device or node.', control: nowInput }),
      ),
    );

    // Attestors.
    const attestors = h('fieldset', { class: 'attestors' }, h('legend', { text: 'Trusted attestors' }));
    const rows = h('div', { class: 'attestor-rows' });
    const addRow = (a = { author: '', kinds: ['attest.identity', 'attest.revoke'], not_after_seq: null }) => {
      const row = h('div', { class: 'attestor-row' });
      const n = rows.children.length;
      const authorIn = h('input', { name: 'att-author', id: `att-author-${n}`, class: 'mono', value: a.author, spellcheck: 'false', autocomplete: 'off', 'aria-label': `Attestor ${n + 1} public key` });
      const kinds = [['attest.identity', 'Identity'], ['attest.domain', 'Shop domains'], ['attest.revoke', 'Revocations']].map(([k, label]) =>
        h('label', { class: 'inline-check' }, h('input', { type: 'checkbox', name: 'att-kind', value: k, checked: (a.kinds || []).includes(k) }), ` ${label}`),
      );
      const cutoff = h('input', { type: 'number', name: 'att-cutoff', min: 0, step: 1, value: a.not_after_seq ?? '', placeholder: 'none', 'aria-label': `Attestor ${n + 1}: ignore records after number` });
      row.append(
        h('div', { class: 'att-key' }, authorIn, a.author ? authorChip(a.author, client.authorView(a.author)?.name ?? null) : null),
        h('div', { class: 'att-kinds', role: 'group', 'aria-label': 'Accepted kinds' }, kinds),
        h('label', { class: 'att-cutoff' }, 'Ignore records after #', cutoff),
        h('button', { type: 'button', class: 'button small', on: { click: () => row.remove() } }, 'Remove'),
      );
      rows.append(row);
    };
    for (const a of policy.trusted_attestors || []) addRow(a);
    attestors.append(
      h('p', { class: 'hint', text: 'An attestor checks real-world identity or domain ownership and signs an attestation. Trust only attestors whose checks you know.' }),
      rows,
      h('button', { type: 'button', class: 'button small', on: { click: () => addRow() } }, 'Add attestor'),
    );
    form.append(attestors);

    const status = h('div', { role: 'status' });
    form.append(
      h('p', { class: 'actions' },
        h('button', { type: 'submit', class: 'button primary', text: 'Apply policy' }),
        ' ',
        h('button', {
          type: 'button',
          class: 'button',
          on: {
            click: () => {
              resetPolicy();
              saveSettings({ pinnedNow: null });
              renderFooter();
              announce('Bundled policy restored.');
              ctx.refresh();
            },
          },
        }, 'Restore bundled policy'),
      ),
      status,
    );
    form.addEventListener('submit', (ev) => {
      ev.preventDefault();
      try {
        const next = readForm(form, policy, pin.checked ? new Date(nowInput.value).getTime() : null);
        savePolicy(next);
        saveSettings({ pinnedNow: next.now });
        renderFooter();
        announce('Policy applied.');
        ctx.refresh();
      } catch (e) {
        status.replaceChildren(notice('bad', 'Policy not applied', h('p', { text: e.message })));
      }
    });
    section.append(form);

    // Compare with a node.
    section.append(nodePolicyPanel(ctx, policy));

    section.append(
      h('details', { class: 'panel' },
        h('summary', { text: 'Policy as JSON' }),
        h('pre', { class: 'json', id: 'policy-json', tabindex: '0', role: 'region', 'aria-label': 'Policy as JSON', text: JSON.stringify(policy, null, 2) }),
      ),
    );
    return section;
  },
};

function readForm(form, base, now) {
  const next = { ...base };
  for (const [k, label] of NUMBER_FIELDS) next[k] = wholeNumber(form.elements[k].value, `"${label}"`);
  const clock = form.elements.clock_minutes;
  if (clock.value.trim() !== clock.dataset.original) {
    const t = clock.value.trim();
    const minutes = Number(t);
    const ms = Math.round(minutes * 60000);
    if (!/^\d+(\.\d+)?$/.test(t) || !Number.isSafeInteger(ms)) throw new Error(`"${CLOCK[1]}" must be a number of minutes of at least 0.`);
    next[CLOCK[0]] = ms;
  }
  const rt = wholeNumber(form.elements.review_threshold.value, '"Flags to hide a review"', 1);
  const pt = wholeNumber(form.elements.product_threshold.value, '"Flags to hide a product record"', 1);
  next.flag_thresholds = { ...(base.flag_thresholds || {}), review: rt, product: pt };
  for (const [k] of BOOL_FIELDS) next[k] = form.elements[k].checked;
  if (now != null && (!Number.isSafeInteger(now) || now < 0)) throw new Error('Enter a valid pinned time.');
  next.now = now;
  const atts = [];
  let n = 0;
  for (const row of form.querySelectorAll('.attestor-row')) {
    n += 1;
    const author = row.querySelector('[name=att-author]').value.trim().toLowerCase();
    if (!author) continue;
    if (!/^[0-9a-f]{64}$/.test(author)) throw new Error(`The key of attestor ${n} must be 64 hex characters.`);
    const kinds = [...row.querySelectorAll('[name=att-kind]:checked')].map((c) => c.value);
    if (!kinds.length) throw new Error(`Attestor ${n} needs at least one accepted kind.`);
    const cut = row.querySelector('[name=att-cutoff]').value.trim();
    atts.push({ author, kinds, not_after_seq: cut === '' ? null : wholeNumber(cut, `"Ignore records after #" of attestor ${n}`) });
  }
  next.trusted_attestors = atts;
  const problem = policyNumberProblem(next);
  if (problem) throw new Error(problem);
  return next;
}

export function diffPolicies(a, b) {
  const na = normalizePolicy(a);
  const nb = normalizePolicy(b);
  const keys = [...new Set([...Object.keys(na), ...Object.keys(nb)])].sort();
  const out = [];
  for (const k of keys) {
    const va = JSON.stringify(na[k]);
    const vb = JSON.stringify(nb[k]);
    if (va !== vb) out.push({ key: k, mine: na[k], theirs: nb[k] });
  }
  return out;
}

function nodePolicyPanel(ctx, policy) {
  const panel = h('section', { class: 'panel', 'aria-labelledby': 'h-nodepolicy' },
    h('h2', { id: 'h-nodepolicy', text: "Compare with a node's policy" }),
    h('p', { text: "Nodes publish a suggested policy. This app never uses it unless you choose to here." }),
  );
  const nodes = activeNodes();
  const sel = h('select', { id: 'np-node' }, nodes.map((u) => h('option', { value: u, text: httpBaseFor(u) || u })));
  const out = h('div', { role: 'status' });
  panel.append(
    field({ id: 'np-node', label: 'Node', control: sel }),
    h('button', {
      type: 'button',
      class: 'button',
      disabled: !nodes.length,
      on: {
        click: async () => {
          const base = httpBaseFor(sel.value);
          out.replaceChildren(h('p', { class: 'muted', text: 'Loading.' }));
          try {
            if (ctx.app.mode === 'mock') throw new Error('The mock client has no real nodes to ask.');
            const res = await fetch(`${base}/v1/policy`, { signal: ctx.signal });
            if (!res.ok) throw new Error(`HTTP ${res.status}`);
            const body = await res.json();
            const theirs = body.policy ?? body;
            const diff = diffPolicies(policy, theirs);
            if (!diff.length) {
              out.replaceChildren(notice('good', 'Same policy', h('p', {}, 'The node suggests the policy you use. Hash ', hex(hashOf(theirs), { n: 16 }), '.')));
              return;
            }
            out.replaceChildren(
              h('div', { class: 'table-scroll' },
                h('table', { class: 'diff' },
                  h('thead', {}, h('tr', {}, h('th', { scope: 'col', text: 'Field' }), h('th', { scope: 'col', text: 'Yours' }), h('th', { scope: 'col', text: "Node's" }))),
                  h('tbody', {}, diff.map((d) => h('tr', {}, h('th', { scope: 'row' }, h('code', { text: d.key })), h('td', {}, h('pre', { class: 'json small', text: JSON.stringify(d.mine, null, 1) ?? 'none' })), h('td', {}, h('pre', { class: 'json small', text: JSON.stringify(d.theirs, null, 1) ?? 'none' }))))),
                ),
              ),
              h('button', {
                type: 'button',
                class: 'button',
                on: {
                  click: () => {
                    const next = { ...theirs, now: policy.now ?? null };
                    const problem = policyNumberProblem(next);
                    try {
                      if (problem) throw new Error(problem);
                      savePolicy(next);
                    } catch (e) {
                      out.replaceChildren(notice('bad', "The node's policy is invalid", h('p', { text: `${e.message || e} Your policy did not change.` })));
                      return;
                    }
                    renderFooter();
                    announce("Using the node's policy.");
                    ctx.refresh();
                  },
                },
              }, "Use this node's policy"),
            );
          } catch (e) {
            out.replaceChildren(notice('bad', 'Could not load the policy', h('p', { text: String(e.message || e) })));
          }
        },
      },
    }, 'Load and compare'),
    out,
  );
  void fmt;
  return panel;
}
