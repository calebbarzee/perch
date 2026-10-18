// Mock signing for the mock client. NOT secure: anyone can forge these
// signatures. It exists so the UI can be developed and tested without wasm,
// while still detecting tampered records (wrong ID, body hash, or signature).

import { sha256Hex, toHex } from './sha256.js';
import { canonicalJson } from './canonical.js';

export const HEX64 = /^[0-9a-f]{64}$/;
export const HEX128 = /^[0-9a-f]{128}$/;
export const KIND_RE = /^[a-z][a-z0-9_.]{0,31}$/;

const HEADER_FIELDS = ['v', 'author', 'seq', 'prev', 'ts', 'kind', 'replaces', 'body_hash'];
const ALL_FIELDS = new Set([...HEADER_FIELDS, 'sig', 'body']);

export function randomHex(bytes) {
  const buf = new Uint8Array(bytes);
  crypto.getRandomValues(buf);
  return toHex(buf);
}

export function mockAuthor(secret) {
  return sha256Hex('perch-mock/pub\n' + secret);
}

export function mockSign(author, message) {
  return (
    sha256Hex('perch-mock/sig-a\n' + author + '\n' + message) +
    sha256Hex('perch-mock/sig-b\n' + author + '\n' + message)
  );
}

export function headerOf(rec) {
  const h = {};
  for (const f of HEADER_FIELDS) if (f in rec) h[f] = rec[f];
  return h;
}

export function recordId(rec) {
  return sha256Hex(canonicalJson(headerOf(rec)));
}

export function makeRecord(secret, { seq, prev, ts, kind, replaces, body }) {
  const author = mockAuthor(secret);
  const header = { v: 1, author, seq, prev, ts, kind, body_hash: sha256Hex(canonicalJson(body)) };
  if (replaces) header.replaces = replaces;
  const hc = canonicalJson(header);
  const sig = mockSign(author, 'perch/record/v1\n' + hc);
  const record = { ...header, sig, body };
  return { id: sha256Hex(hc), record };
}

export function receiptMessage(r) {
  return (
    'perch/receipt/v1\n' +
    canonicalJson({ listing: r.listing, buyer: r.buyer, purchased_at: r.purchased_at, nonce: r.nonce })
  );
}

export function makeReceipt(sellerSecret, listing, buyer, purchasedAt, nonce = randomHex(32)) {
  const r = { listing, buyer, purchased_at: purchasedAt, nonce };
  r.sig = mockSign(mockAuthor(sellerSecret), receiptMessage(r));
  return r;
}

// Isolated validity (ARCHITECTURE 4.4), mock signature scheme.
export function verifyRecordMock(input) {
  let rec = input;
  try {
    if (typeof input === 'string') {
      if (new TextEncoder().encode(input).length > 131072) {
        return { valid: false, id: null, error: 'Record line is larger than 131,072 bytes.' };
      }
      rec = JSON.parse(input);
    }
  } catch (e) {
    return { valid: false, id: null, error: `Not valid JSON: ${e.message}` };
  }
  const fail = (error, id = null) => ({ valid: false, id, error });
  if (!rec || typeof rec !== 'object' || Array.isArray(rec)) return fail('Record must be a JSON object.');
  for (const k of Object.keys(rec)) if (!ALL_FIELDS.has(k)) return fail(`Unknown field "${k}".`);
  if (rec.v !== 1) return fail('Field "v" must be 1.');
  if (typeof rec.author !== 'string' || !HEX64.test(rec.author)) return fail('Field "author" must be 64 lowercase hex characters.');
  if (!Number.isSafeInteger(rec.seq) || rec.seq < 1) return fail('Field "seq" must be an integer of at least 1.');
  if (rec.seq === 1 ? rec.prev !== null : typeof rec.prev !== 'string' || !HEX64.test(rec.prev)) {
    return fail('Field "prev" must be null for seq 1 and a record ID otherwise.');
  }
  if (!Number.isSafeInteger(rec.ts) || rec.ts < 0) return fail('Field "ts" must be a non-negative integer.');
  if (typeof rec.kind !== 'string' || !KIND_RE.test(rec.kind)) return fail('Field "kind" has an invalid format.');
  if ('replaces' in rec && (typeof rec.replaces !== 'string' || !HEX64.test(rec.replaces))) {
    return fail('Field "replaces" must be a record ID.');
  }
  if (typeof rec.body_hash !== 'string' || !HEX64.test(rec.body_hash)) return fail('Field "body_hash" must be 64 lowercase hex characters.');
  if (typeof rec.sig !== 'string' || !HEX128.test(rec.sig)) return fail('Field "sig" must be 128 lowercase hex characters.');
  let hc;
  try {
    hc = canonicalJson(headerOf(rec));
  } catch (e) {
    return fail(e.message);
  }
  const id = sha256Hex(hc);
  if ('body' in rec) {
    if (!rec.body || typeof rec.body !== 'object' || Array.isArray(rec.body)) return fail('Field "body" must be an object.', id);
    let bc;
    try {
      bc = canonicalJson(rec.body);
    } catch (e) {
      return fail(`Body: ${e.message}`, id);
    }
    if (sha256Hex(bc) !== rec.body_hash) return fail('Body does not match body_hash.', id);
  }
  if (mockSign(rec.author, 'perch/record/v1\n' + hc) !== rec.sig) return fail('Signature does not verify.', id);
  return { valid: true, id, error: null };
}
