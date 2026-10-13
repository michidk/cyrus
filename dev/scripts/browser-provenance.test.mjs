import { test } from 'node:test';
import assert from 'node:assert/strict';
import { verifyBrowserArtifacts } from './browser-provenance.mjs';

const names = [
  'index.html', 'loading.css', 'loader.js', 'install.js', 'local-install.js',
  'game-assets.js', 'asset-worker.js', 'pkg/cyrus-web.js', 'pkg/cyrus-web_bg.wasm',
  'pkg/cyrus-assets-worker.js', 'pkg/cyrus-assets-worker_bg.wasm',
].map(name => 'target/web/' + name);
const build = {version: 1, artifacts: Object.fromEntries(names.map(name => [name, 'digest:' + name]))};
const observed = Object.fromEntries(names.map(name => [name, {sha256: 'digest:' + name}]));

test('loaded browser code must cover both application and worker artifacts', () => {
  assert.equal(verifyBrowserArtifacts(build, observed).build, build);
  const missing = {...observed};
  delete missing['target/web/pkg/cyrus-assets-worker_bg.wasm'];
  assert.throws(() => verifyBrowserArtifacts(build, missing), /did not observe loaded artifact/);
});
test('stale code and unexpected same-origin scripts cannot borrow build provenance', () => {
  const stale = {...observed, 'target/web/loader.js': {sha256: 'other'}};
  assert.throws(() => verifyBrowserArtifacts(build, stale), /loader.js/);
  const unexpected = {...observed, 'target/web/unknown.js': {sha256: 'other'}};
  assert.throws(() => verifyBrowserArtifacts(build, unexpected), /unknown.js/);
});
