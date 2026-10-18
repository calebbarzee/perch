// Device storage for the key backup and the record set.
//
// IndexedDB database "perch" (or "perch-mock" for the mock client, whose
// signatures are fake and must never mix with real data), object store "kv":
//   key      -> exportKey() backup JSON, only if the user keeps the key here
//   records  -> JSONL export of the local store
// localStorage "perch.head.v1" keeps the user's highest known own head even
// when the key itself is not kept, so a later import cannot publish below it.
//
// Limits: browsers give each origin a quota (Chromium: up to 60 % of free disk;
// Firefox: up to 10 GiB; Safari: about 1 GiB before asking). Records are
// stored as one JSONL string, rewritten at most once per second. Records are
// re-verified by the client when loaded, so edits to storage are rejected.

const STORE = 'kv';
const HEAD_KEY = 'perch.head.v1';

let dbPromise = null;
let dbName = 'perch';

export function useNamespace(mode) {
  dbName = mode === 'mock' ? 'perch-mock' : 'perch';
  dbPromise = null;
}

function db() {
  if (!dbPromise) {
    dbPromise = new Promise((resolve, reject) => {
      if (typeof indexedDB === 'undefined') {
        reject(new Error('IndexedDB is not available'));
        return;
      }
      const req = indexedDB.open(dbName, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(STORE);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
      req.onblocked = () => reject(new Error('IndexedDB is blocked by another tab'));
    });
    dbPromise.catch(() => {
      dbPromise = null;
    });
  }
  return dbPromise;
}

async function tx(mode, fn) {
  const d = await db();
  return new Promise((resolve, reject) => {
    const t = d.transaction(STORE, mode);
    const store = t.objectStore(STORE);
    const req = fn(store);
    t.oncomplete = () => resolve(req ? req.result : undefined);
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

export async function get(key) {
  try {
    return await tx('readonly', (s) => s.get(key));
  } catch {
    return undefined;
  }
}

export async function set(key, value) {
  try {
    await tx('readwrite', (s) => s.put(value, key));
    return true;
  } catch {
    return false;
  }
}

export async function del(key) {
  try {
    await tx('readwrite', (s) => s.delete(key));
    return true;
  } catch {
    return false;
  }
}

export const loadKey = () => get('key');
export const saveKey = (backupJson) => set('key', backupJson);
export const forgetKey = () => del('key');
export const loadRecords = () => get('records');

// Debounced record save.
let pending = null;
let timer = null;
export function scheduleSaveRecords(getJsonl) {
  pending = getJsonl;
  if (timer) return;
  timer = setTimeout(async () => {
    timer = null;
    const fn = pending;
    pending = null;
    if (fn) await set('records', fn());
  }, 1000);
}

export async function flushRecords(getJsonl) {
  if (timer) {
    clearTimeout(timer);
    timer = null;
  }
  pending = null;
  await set('records', getJsonl());
}

export async function clearAll() {
  await del('records');
  await del('key');
}

// Highest own head per author, kept even when the key is not.
export function knownHead(author) {
  try {
    const all = JSON.parse(localStorage.getItem(HEAD_KEY) || '{}');
    return all[author] ?? null;
  } catch {
    return null;
  }
}

export function rememberHead(author, head) {
  if (!author || !head) return;
  try {
    const all = JSON.parse(localStorage.getItem(HEAD_KEY) || '{}');
    if (!all[author] || all[author].seq < head.seq) {
      all[author] = head;
      localStorage.setItem(HEAD_KEY, JSON.stringify(all));
    }
  } catch {
    // Storage blocked: the head lives in the in-memory client only.
  }
}

// Merge a backup with the highest head this device has seen for that author.
export function mergeBackupHead(backupJson, author) {
  let backup;
  try {
    backup = JSON.parse(backupJson);
  } catch {
    return backupJson;
  }
  const known = knownHead(author);
  if (known && (!backup.head || backup.head.seq < known.seq)) backup.head = known;
  return JSON.stringify(backup);
}
