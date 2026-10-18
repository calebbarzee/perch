// Starts and stops perch-node processes for browser tests.

import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

export const REPO = path.resolve(path.dirname(new URL(import.meta.url).pathname), '../../..');

export function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.unref();
    s.on('error', reject);
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function waitFor(fn, { timeout = 20000, interval = 200, what = 'condition' } = {}) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (e) {
      last = e;
    }
    await sleep(interval);
  }
  throw new Error(`timed out waiting for ${what}${last ? `: ${last.message}` : ''}`);
}

// Returns the path of a perch-node binary, building it if needed.
export function nodeBinary({ build = true } = {}) {
  for (const p of ['target/release/perch-node', 'target/debug/perch-node']) {
    const full = path.join(REPO, p);
    if (fs.existsSync(full)) return full;
  }
  if (!build) return null;
  console.log('Building perch-node (cargo build -p perch-node).');
  const r = spawnSync('cargo', ['build', '-p', 'perch-node'], { cwd: REPO, stdio: 'inherit' });
  if (r.status !== 0) throw new Error('cargo build -p perch-node failed');
  return path.join(REPO, 'target/debug/perch-node');
}

// perch-node demo --out <file> [--policy-out <file>]
export function generateDemo(bin, dir) {
  const out = path.join(dir, 'demo.jsonl');
  const policyOut = path.join(dir, 'demo-policy.json');
  let r = spawnSync(bin, ['demo', '--out', out, '--policy-out', policyOut], { encoding: 'utf8' });
  if (r.status !== 0) {
    // Older CLI without --policy-out.
    r = spawnSync(bin, ['demo', '--out', out], { encoding: 'utf8' });
    if (r.status !== 0) throw new Error(`perch-node demo failed: ${r.stderr}`);
  }
  const policy = fs.existsSync(policyOut) ? JSON.parse(fs.readFileSync(policyOut, 'utf8')) : null;
  return { jsonl: fs.readFileSync(out, 'utf8'), file: out, policy };
}

export class PerchNode {
  constructor({ bin, name, port, dataDir, peers = [], attestors = [], extraArgs = [] }) {
    Object.assign(this, { bin, name, port, dataDir, peers, attestors, extraArgs });
    this.http = `http://127.0.0.1:${port}`;
    this.ws = `ws://127.0.0.1:${port}/v1/sync`;
    this.logFile = path.join(dataDir, '..', `${name}.log`);
    this.proc = null;
  }

  async start() {
    fs.mkdirSync(this.dataDir, { recursive: true });
    const args = ['serve', '--listen', `127.0.0.1:${this.port}`, '--data', this.dataDir];
    for (const p of this.peers) args.push('--peer', p);
    for (const a of this.attestors) args.push('--attestor', a);
    args.push(...this.extraArgs);
    const log = fs.openSync(this.logFile, 'a');
    this.proc = spawn(this.bin, args, { stdio: ['ignore', log, log], env: { ...process.env, RUST_LOG: process.env.RUST_LOG || 'warn' } });
    this.proc.on('exit', (code, sig) => {
      this.exited = { code, sig };
    });
    await waitFor(async () => {
      if (this.exited) throw new Error(`${this.name} exited (${JSON.stringify(this.exited)}); see ${this.logFile}`);
      const r = await fetch(`${this.http}/v1/status`);
      return r.ok;
    }, { what: `${this.name} /v1/status` });
    return this;
  }

  async stop() {
    if (!this.proc || this.exited) return;
    this.proc.kill('SIGTERM');
    await waitFor(() => this.exited, { timeout: 5000, what: `${this.name} exit` }).catch(() => this.proc.kill('SIGKILL'));
  }

  async get(p, { json = true } = {}) {
    const r = await fetch(`${this.http}${p}`);
    if (!r.ok) throw new Error(`GET ${p}: HTTP ${r.status}`);
    return json ? r.json() : r.text();
  }

  async getWithHeaders(p) {
    const r = await fetch(`${this.http}${p}`);
    if (!r.ok) throw new Error(`GET ${p}: HTTP ${r.status}`);
    return { body: await r.json(), headers: r.headers };
  }

  // Submits JSONL in chunks within the API limits (1,000 records, 4 MiB),
  // retrying on 429.
  async submit(jsonl) {
    const lines = jsonl.split('\n').filter(Boolean);
    const results = [];
    let i = 0;
    while (i < lines.length) {
      const chunk = [];
      let bytes = 0;
      while (i < lines.length && chunk.length < 1000 && bytes + lines[i].length + 1 < 4 * 1024 * 1024 - 1024) {
        bytes += lines[i].length + 1;
        chunk.push(lines[i++]);
      }
      for (let attempt = 0; ; attempt++) {
        const r = await fetch(`${this.http}/v1/records`, {
          method: 'POST',
          headers: { 'content-type': 'application/x-ndjson' },
          body: chunk.join('\n') + '\n',
        });
        if (r.status === 429 && attempt < 30) {
          await sleep(600);
          continue;
        }
        if (!r.ok) throw new Error(`POST /v1/records: HTTP ${r.status} ${await r.text()}`);
        results.push(await r.json());
        break;
      }
    }
    return results;
  }

  async stats() {
    return this.get('/v1/stats');
  }
}

export function tempDir(prefix = 'perch-browser-') {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}
