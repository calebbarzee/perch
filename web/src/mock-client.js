// Mock PerchClient. Same API as the wasm client (ARCHITECTURE 8.2), backed by
// JavaScript models of the views and a fake signature scheme. Use it for UI
// work and tests without the wasm package. Never use it with real data:
// its signatures can be forged by anyone.

import { canonicalJson, normalizePolicy } from './canonical.js';
import { sha256Hex } from './sha256.js';
import { buildMockDataset } from './mock-data.js';
import { MockStore, computeState, searchState } from './mock-views.js';
import { CURRENCIES } from './mock-currencies.js';
import {
  HEX64,
  KIND_RE,
  makeRecord,
  makeReceipt,
  mockAuthor,
  mockSign,
  randomHex,
  receiptMessage,
  verifyRecordMock,
} from './mock-crypto.js';

let dataset = null;
const getDataset = () => (dataset ??= buildMockDataset());

export function mockPolicy() {
  return structuredClone(getDataset().policy);
}

function parseInput(recordsOrJsonl) {
  if (typeof recordsOrJsonl === 'string') {
    const lines = recordsOrJsonl.split('\n').map((l) => l.trim()).filter(Boolean);
    if (lines.length === 1 && lines[0].startsWith('[')) {
      try {
        return JSON.parse(lines[0]).map((r) => JSON.stringify(r));
      } catch {
        return lines;
      }
    }
    return lines;
  }
  if (Array.isArray(recordsOrJsonl)) return recordsOrJsonl;
  return [recordsOrJsonl];
}

// How long a node may take to send the records for a head it claims
// (guard.rs CLAIM_TIMEOUT_MS). Tests may shorten it.
const claimTimeoutMs = () => globalThis.PERCH_MOCK_CLAIM_TIMEOUT_MS ?? 60000;

export class PerchClient {
  static async create(options = {}) {
    return new PerchClient(options);
  }

  constructor(options = {}) {
    this.sparse = Boolean(options.sparse);
    this.store = new MockStore({ sparse: this.sparse });
    this.version = 0;
    this.listeners = new Set();
    this.secret = null;
    this.ownHead = null; // highest known own head { seq, id }
    this.needsSync = false; // imported key: sync every node first
    this.peerMap = new Map();
    this._policy = mockPolicy();
    this.cache = null;
  }

  // Keys.

  generateKey() {
    this.secret = randomHex(32);
    this.ownHead = null;
    this.needsSync = false;
    for (const p of this.peerMap.values()) {
      p.claim = null;
      if (p.state === 'live') this.noteClaim(p);
    }
    this.emit('key');
    return this.author();
  }

  importKey(backupOrSecretHex) {
    let secret = backupOrSecretHex;
    let head = null;
    const text = String(backupOrSecretHex).trim();
    if (text.startsWith('{')) {
      let parsed;
      try {
        parsed = JSON.parse(text);
      } catch {
        throw new Error('The key backup is not valid JSON.');
      }
      secret = parsed.secret;
      head = parsed.head ?? null;
      if (head && (!Number.isSafeInteger(head.seq) || !HEX64.test(head.id || ''))) {
        throw new Error('The key backup has an invalid head.');
      }
    } else {
      secret = text;
    }
    if (typeof secret !== 'string' || !HEX64.test(secret)) {
      throw new Error('A secret key is 64 lowercase hex characters.');
    }
    this.secret = secret;
    this.ownHead = head;
    this.needsSync = true;
    this.refreshOwnHead();
    for (const p of this.peerMap.values()) {
      p.syncedSinceImport = false;
      p.claim = null;
    }
    // Peers that are already live sync the imported author again.
    for (const p of this.peerMap.values()) if (p.state === 'live') this.simulateSync(p);
    this.emit('key');
    return this.author();
  }

  exportKey() {
    if (!this.secret) throw new Error('No key. Generate or import a key first.');
    this.refreshOwnHead();
    return JSON.stringify({ secret: this.secret, head: this.ownHead });
  }

  author() {
    return this.secret ? mockAuthor(this.secret) : null;
  }

  refreshOwnHead() {
    const a = this.author();
    if (!a) return;
    const local = this.store.head(a);
    if (local && (!this.ownHead || local.seq > this.ownHead.seq)) this.ownHead = local;
  }

  publishBlocker() {
    if (!this.secret) return 'No key. Generate or import a key first.';
    if (this.sparse) return 'This client has a sparse store and cannot publish.';
    if (this.needsSync) {
      const peers = [...this.peerMap.values()];
      if (!peers.length) return 'This key was imported. Connect to a node and wait for sync before publishing.';
      const pending = peers.filter((p) => !p.syncedSinceImport);
      if (pending.length) return `This key was imported. Waiting for sync from ${pending.length} of ${peers.length} nodes.`;
    }
    const a = this.author();
    const local = this.store.head(a);
    if (this.ownHead && (!local || local.seq < this.ownHead.seq)) {
      return `Your log has ${this.ownHead.seq} records, but this device holds ${local ? local.seq : 0}. Sync the missing records from a node first.`;
    }
    // A node URL containing "withhold" simulates a node that claims more
    // records of the user's log than it sends (guard.rs).
    const localSeq = local ? local.seq : 0;
    const claims = [...this.peerMap.values()].filter((p) => p.claim && p.claim.head.seq > localSeq);
    claims.sort((x, y) => Number(this.unproven(y)) - Number(this.unproven(x)));
    if (claims.length) {
      const c = claims[0];
      const host = new URL(c.url).host;
      const what = `${c.claim.head.seq} records in your log; this device holds ${localSeq}`;
      return this.unproven(c)
        ? `Node ${host} claims ${what}. It did not send the records in ${Math.round(claimTimeoutMs() / 1000)} s. Retry, or stop using this node.`
        : `Node ${host} claims ${what}. Waiting for the node to send the records.`;
    }
    return null;
  }

  unproven(peer) {
    return Boolean(peer.claim) && Date.now() - peer.claim.since >= claimTimeoutMs();
  }

  noteClaim(peer) {
    const a = this.author();
    if (!a || !peer.url.includes('withhold') || peer.claim) return;
    const local = this.store.head(a);
    peer.claim = { head: { seq: (local?.seq ?? 0) + 3, id: 'f'.repeat(64) }, since: Date.now() };
  }

  // Non-contract helper, as in the wasm client.
  retryNode(wsUrl) {
    const p = this.peerMap.get(wsUrl);
    if (p?.claim) p.claim.since = Date.now();
    this.emit('peers');
  }

  publishStatus() {
    const reason = this.publishBlocker();
    const a = this.author();
    const local = a ? this.store.head(a) : null;
    const nodes = [...this.peerMap.values()].map((p) => {
      let head = null;
      if (a && p.state === 'live') {
        for (const r of p.node.records.values()) {
          if (r.author === a && (!head || r.seq > head.seq)) head = { seq: r.seq, id: this.store.idOf(r) };
        }
      }
      const claimed = p.claim && p.claim.head.seq > (local?.seq ?? 0);
      if (claimed) head = p.claim.head;
      return { url: p.url, head, checked: this.needsSync ? p.syncedSinceImport : p.state === 'live', unproven: Boolean(claimed && this.unproven(p)) };
    });
    return { ready: reason === null, reason, local_head: a ? this.store.head(a) : null, nodes };
  }

  publish(kind, body, opts = {}) {
    const blocker = this.publishBlocker();
    if (blocker) throw new Error(blocker);
    if (typeof kind !== 'string' || !KIND_RE.test(kind)) throw new Error(`Invalid kind "${kind}".`);
    if (!body || typeof body !== 'object' || Array.isArray(body)) throw new Error('Body must be an object.');
    const a = this.author();
    const head = this.store.head(a);
    const seq = (head?.seq ?? 0) + 1;
    const prevRec = head ? this.store.byId.get(head.id) : null;
    const ts = Math.max(opts.ts ?? Date.now(), prevRec?.ts ?? 0);
    const { id, record } = makeRecord(this.secret, {
      seq,
      prev: head ? head.id : null,
      ts,
      kind,
      replaces: opts.replaces,
      body: JSON.parse(JSON.stringify(body)),
    });
    const res = this.store.insert(record);
    if (res.outcome !== 'inserted') throw new Error(`Publish failed: ${res.outcome}`);
    this.ownHead = { seq, id };
    this.bump();
    for (const p of this.peerMap.values()) {
      if (p.state === 'live') {
        p.sent += 1;
        p.node.records.set(id, record);
      }
    }
    return { id, record };
  }

  issueReceipt(listingRootId, buyerAuthor, purchasedAt) {
    if (!this.secret) throw new Error('No key. Generate or import a key first.');
    if (!HEX64.test(listingRootId || '')) throw new Error('Listing ID must be 64 lowercase hex characters.');
    if (!HEX64.test(buyerAuthor || '')) throw new Error("Buyer key must be 64 lowercase hex characters.");
    if (!Number.isSafeInteger(purchasedAt)) throw new Error('Purchase time must be Unix milliseconds.');
    return makeReceipt(this.secret, listingRootId, buyerAuthor, purchasedAt);
  }

  verifyReceipt(receipt) {
    let r = receipt;
    try {
      if (typeof r === 'string') r = JSON.parse(r);
    } catch {
      return { valid: false, seller: null, error: 'The receipt is not valid JSON.' };
    }
    const keys = Object.keys(r || {}).sort().join(',');
    if (keys !== 'buyer,listing,nonce,purchased_at,sig') {
      return { valid: false, seller: null, error: 'A receipt has exactly the fields listing, buyer, purchased_at, nonce, and sig.' };
    }
    const rec = this.store.byId.get(r.listing);
    if (!rec || rec.kind !== 'listing') {
      return { valid: false, seller: null, error: 'The listing is not in your local data. Sync with a node and try again.' };
    }
    if (mockSign(rec.author, receiptMessage(r)) !== r.sig) {
      return { valid: false, seller: rec.author, error: "The signature does not match the listing's seller." };
    }
    return { valid: true, seller: rec.author, error: null };
  }

  // Records.

  ingest(recordsOrJsonl) {
    const out = { inserted: 0, duplicate: 0, fork: 0, gap: 0, rejected: 0, errors: [] };
    const items = parseInput(recordsOrJsonl);
    items.forEach((item, index) => {
      let rec = item;
      if (typeof item === 'string') {
        try {
          rec = JSON.parse(item);
        } catch (e) {
          out.rejected++;
          out.errors.push({ index, id: null, error: `Not valid JSON: ${e.message}` });
          return;
        }
      }
      const res = this.store.insert(rec);
      out[res.outcome]++;
      if (res.outcome === 'rejected') out.errors.push({ index, id: res.id ?? null, error: res.error });
    });
    if (out.inserted || out.fork) {
      this.refreshOwnHead();
      this.bump();
    }
    return out;
  }

  record(id) {
    const r = this.store.byId.get(id);
    return r ? structuredClone(r) : null;
  }

  verifyRecord(json) {
    return verifyRecordMock(json);
  }

  exportJsonl() {
    return this.store.all().map((r) => canonicalJson(r)).join('\n') + (this.store.byId.size ? '\n' : '');
  }

  // Policy and views.

  setPolicy(policy) {
    if (!policy || typeof policy !== 'object') throw new Error('Policy must be an object.');
    if (!Array.isArray(policy.trusted_attestors)) throw new Error('Policy needs a trusted_attestors list.');
    this._policy = structuredClone(policy);
    this.cache = null;
    this.emit('policy');
  }

  policy() {
    return structuredClone(this._policy);
  }

  policyHash() {
    return sha256Hex(canonicalJson(normalizePolicy(this._policy)));
  }

  state() {
    const policy = { ...this._policy };
    if (policy.now == null) policy.now = Date.now();
    // Cache for one second when now is not pinned, so a render pass is consistent.
    const stamp = this._policy.now == null ? Math.floor(policy.now / 1000) : 'pinned';
    if (this.cache && this.cache.version === this.version && this.cache.stamp === stamp) return this.cache.state;
    const state = computeState(this.store, policy);
    this.cache = { version: this.version, stamp, state };
    return state;
  }

  search(q, opts = {}) {
    return structuredClone(searchState(this.state(), q, opts));
  }

  product(key) {
    const v = this.state().productViews.get(key);
    return v ? structuredClone(v) : null;
  }

  listing(id) {
    const v = this.state().listingViews.get(id);
    return v ? structuredClone(v) : null;
  }

  authorView(id) {
    const v = this.state().authorViews.get(id);
    return v ? structuredClone(v) : null;
  }

  stats() {
    return structuredClone(this.state().stats);
  }

  authorRecords(author, { kinds, limit, from_seq } = {}) {
    const st = this.state();
    const out = [];
    for (const rt of st.chains.keys()) {
      const sum = st.chainSummary(rt);
      if (!sum || sum._author !== author) continue;
      if (kinds && !kinds.includes(sum.kind)) continue;
      if (from_seq != null && sum._seq < from_seq) continue;
      out.push(sum);
    }
    out.sort((a, b) => b._seq - a._seq || (a.root < b.root ? -1 : 1));
    const list = out.slice(0, limit ?? out.length);
    for (const x of list) {
      delete x._seq;
      delete x._author;
    }
    return list;
  }

  flags(rootId) {
    return structuredClone(this.state().flagsFor(rootId));
  }

  recordStatus(id) {
    return structuredClone(this.state().recordStatus(id));
  }

  currencies() {
    return CURRENCIES.map((c) => ({ ...c }));
  }

  authorLog(author) {
    return (this.store.logs.get(author) || []).filter(Boolean).map((id) => structuredClone(this.store.byId.get(id)));
  }

  buildInfo() {
    return { version: '0.1.0-mock', wasm_sha256: null };
  }

  // Sync (simulated). Each URL is a fake node that holds the mock dataset.
  // The second node URL lacks the newest records of two authors, so the
  // Network screen can show a head disagreement.

  async connect(wsUrl) {
    if (this.peerMap.has(wsUrl)) return;
    const index = this.peerMap.size;
    const data = getDataset();
    const records = new Map();
    let dropped = new Set();
    if (index === 1) {
      const lastOf = new Map();
      data.records.forEach((r, i) => lastOf.set(r.author, i));
      const authors = [...lastOf.keys()].sort().slice(0, 2);
      dropped = new Set(authors.map((a) => lastOf.get(a)));
    }
    data.records.forEach((r, i) => {
      if (!dropped.has(i)) records.set(verifyRecordMock(r).id || String(i), r);
    });
    const peer = {
      url: wsUrl,
      state: 'connecting',
      sent: 0,
      received: 0,
      node: { records },
      syncedSinceImport: false,
    };
    this.peerMap.set(wsUrl, peer);
    this.emit('peers');
    await new Promise((r) => setTimeout(r, 120));
    if (!this.peerMap.has(wsUrl)) return;
    peer.state = 'syncing';
    this.emit('peers');
    await new Promise((r) => setTimeout(r, 120));
    if (!this.peerMap.has(wsUrl)) return;
    this.simulateSync(peer);
  }

  simulateSync(peer) {
    const res = this.ingest([...peer.node.records.values()]);
    peer.received += res.inserted + res.duplicate + res.fork;
    // Push records the node lacks.
    for (const r of this.store.all()) {
      const id = this.store.idOf(r);
      if (!peer.node.records.has(id) && r.author === this.author()) {
        peer.node.records.set(id, r);
        peer.sent++;
      }
    }
    peer.state = 'live';
    peer.syncedSinceImport = true;
    this.refreshOwnHead();
    this.noteClaim(peer);
    this.emit('peers');
  }

  async disconnect(wsUrl) {
    if (!this.peerMap.delete(wsUrl)) return;
    this.emit('peers');
  }

  peers() {
    const localHeads = this.store.heads();
    return [...this.peerMap.values()].map((p) => {
      const nodeHeads = new Map();
      for (const r of p.node.records.values()) {
        const prev = nodeHeads.get(r.author) ?? 0;
        if (r.seq > prev) nodeHeads.set(r.author, r.seq);
      }
      let behind = 0;
      let ahead = 0;
      if (p.state === 'live') {
        for (const [a, hd] of Object.entries(localHeads)) {
          const n = nodeHeads.get(a) ?? 0;
          if (n < hd.seq) behind++;
        }
        for (const [a, n] of nodeHeads) if (n > (localHeads[a]?.seq ?? 0) && !this.store.forkSeq.has(a)) ahead++;
      }
      return { url: p.url, state: p.state, sent: p.sent, received: p.received, behind, ahead };
    });
  }

  // Change notification.

  onChange(callback) {
    this.listeners.add(callback);
    return () => this.listeners.delete(callback);
  }

  bump() {
    this.version++;
    this.cache = null;
    this.emit('records');
  }

  emit(reason) {
    for (const fn of [...this.listeners]) {
      try {
        fn({ reason });
      } catch (e) {
        console.error(e);
      }
    }
  }
}

export default PerchClient;
