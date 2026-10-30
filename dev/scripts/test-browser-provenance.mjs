// Run against a disposable Chromium CDP endpoint; serves only synthetic test bytes.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import http from 'node:http';
import { connectCdp } from './capture-cdp.mjs';
import { observeBrowserBuild } from './browser-provenance.mjs';

assert(process.env.CYRUS_PROVENANCE_CDP, 'set CYRUS_PROVENANCE_CDP');
assert(process.env.CYRUS_PROVENANCE_HOST, 'set CYRUS_PROVENANCE_HOST to the test host reachable by Chromium');
const files = {
  'index.html': '<!doctype html><link rel="stylesheet" href="loading.css"><script type="module" src="loader.js"></script>',
  'sw.js': "self.addEventListener('install', () => self.skipWaiting()); self.addEventListener('activate', event => event.waitUntil(self.clients.claim()));",
  'loading.css': 'body { background: black; }',
  'loader.js': `import './install.js'; import './game-assets.js'; import {start} from './pkg/cyrus-web.js';
    if ('serviceWorker' in navigator) {
      await navigator.serviceWorker.register('./sw.js');
      await navigator.serviceWorker.ready;
      window.cacheReady = true;
    }
    await start(); const worker = new Worker('./asset-worker.js', {type:'module'});
    worker.onmessage = () => { window.ready = true; };`,
  'install.js': "import './local-install.js';",
  'local-install.js': 'export const local = true;',
  'game-assets.js': 'export const game = true;',
  'asset-worker.js': "import {start} from './pkg/cyrus-assets-worker.js'; await start(); postMessage('ready');",
  'pkg/cyrus-web.js': "export async function start() { await WebAssembly.compile(await (await fetch('./pkg/cyrus-web_bg.wasm')).arrayBuffer()); }",
  'pkg/cyrus-assets-worker.js': "export async function start() { await WebAssembly.compile(await (await fetch('./pkg/cyrus-assets-worker_bg.wasm')).arrayBuffer()); }",
  'pkg/cyrus-web_bg.wasm': Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]),
  'pkg/cyrus-assets-worker_bg.wasm': Buffer.from([0, 97, 115, 109, 1, 0, 0, 0, 0, 2, 1, 120]),
};
const build = {version: 1, artifacts: Object.fromEntries(Object.entries(files).map(([name, bytes]) => [
  'target/web/' + name, createHash('sha256').update(bytes).digest('hex'),
]))};
const server = http.createServer((req, res) => {
  const name = new URL(req.url, 'http://test').pathname.slice(1) || 'index.html';
  const bytes = files[name];
  if (bytes === undefined) { res.writeHead(404); res.end(); return; }
  res.setHeader('Content-Type', name.endsWith('.js') ? 'text/javascript' : name.endsWith('.wasm') ? 'application/wasm' : name.endsWith('.css') ? 'text/css' : 'text/html');
  res.end(bytes);
});
await new Promise(resolve => server.listen(Number(process.env.CYRUS_PROVENANCE_PORT || 0), '0.0.0.0', resolve));
try {
  const url = `http://${process.env.CYRUS_PROVENANCE_HOST}:${server.address().port}/`;
  for (const stale of ['none', 'javascript', 'wasm']) {
    files['local-install.js'] = 'export const local = true;';
    if (stale === 'javascript') files['local-install.js'] += '\nexport const changed = true;';
    if (stale === 'wasm') files['pkg/cyrus-assets-worker_bg.wasm'] = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0, 0, 2, 1, 121]);
    const cdp = await connectCdp(process.env.CYRUS_PROVENANCE_CDP);
    try {
      const observation = await observeBrowserBuild(cdp, build, url);
      await cdp.call('Page.navigate', {url});
      const deadline = Date.now() + 10000;
      for (;;) {
        const value = await cdp.call('Runtime.evaluate', {expression: 'window.ready', returnByValue: true});
        if (value.result.value) {
          const cache = await cdp.call('Runtime.evaluate', {expression: '!isSecureContext || window.cacheReady === true', returnByValue: true});
          assert.equal(cache.result.value, true, 'secure startup must activate the service worker');
          break;
        }
        assert(Date.now() < deadline, 'synthetic page did not become ready');
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      if (stale !== 'none') await assert.rejects(observation.verify(), stale === 'javascript' ? /local-install.js/ : /loaded WASM/);
      else {
        const verified = await observation.verify();
        assert.equal(Object.keys(verified.observed).length, Object.keys(files).length - 1);
      }
      observation.stop();
      console.log(stale === 'none' ? 'Verified: page/worker code, WASM, HTML and CSS match build bytes' : `Verified: executed stale ${stale} is rejected`);
    } finally { await cdp.close(); }
  }
} finally { await new Promise(resolve => server.close(resolve)); }
