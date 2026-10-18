// Minimal static file server for tests and local development.
// Usage: node tests/browser/lib/static-server.mjs [dir] [port]
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.woff2': 'font/woff2',
  '.png': 'image/png',
  '.txt': 'text/plain; charset=utf-8',
  '.jsonl': 'application/x-ndjson',
};

export function startStaticServer(root, port = 0, host = '127.0.0.1') {
  const base = path.resolve(root);
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    let p = path.normalize(path.join(base, decodeURIComponent(url.pathname)));
    if (!p.startsWith(base)) {
      res.writeHead(403).end();
      return;
    }
    if (fs.existsSync(p) && fs.statSync(p).isDirectory()) p = path.join(p, 'index.html');
    fs.readFile(p, (err, data) => {
      if (err) {
        res.writeHead(404, { 'content-type': 'text/plain' }).end('not found');
        return;
      }
      res.writeHead(200, {
        'content-type': TYPES[path.extname(p)] || 'application/octet-stream',
        'cache-control': 'no-store',
      });
      res.end(data);
    });
  });
  return new Promise((resolve) => {
    server.listen(port, host, () => resolve({ server, url: `http://${host}:${server.address().port}` }));
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const dir = process.argv[2] || path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../web');
  const port = Number(process.argv[3] || 8080);
  const { url } = await startStaticServer(dir, port);
  console.log(`serving ${dir} at ${url}`);
}
