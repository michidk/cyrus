// Exercise the published WASM game facade against a read-only install, without native exports.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const packageDir = path.resolve(process.argv[2] || 'target/web/pkg');
const install = path.resolve(process.argv[3] || process.env.REDGUARD_DIR || 'redguard');
if (!fs.existsSync(path.join(install, 'RGFX.EXE'))) {
  console.log('skipped: no install (WASM game worker)');
  process.exit(0);
}
const root = fs.realpathSync(install), originals = new Map();
function scan(directory, prefix = '') {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name), name = prefix + entry.name;
    if (entry.isDirectory()) scan(filename, name + '/');
    else if (entry.isFile()) originals.set(name, filename);
  }
}
scan(root);
for (const name of ['game.gog', 'game.ins']) {
  const filename = path.join(root, '..', name);
  if (fs.existsSync(filename)) originals.set('disc/' + name, filename);
}
const splash = path.join(root, '../DOSBOX/3DfxSpl2.dll');
if (fs.existsSync(splash)) originals.set('splash/3DfxSpl2.dll', splash);
const requests = [];
globalThis.XMLHttpRequest = class {
  open(method, url, async) { assert.equal(method, 'GET'); assert.equal(async, false); this.url = new URL(url); }
  setRequestHeader(name, value) { assert.equal(name, 'Range'); this.range = value; }
  getResponseHeader(name) { assert.equal(name, 'Content-Range'); return this.contentRange; }
  send() {
    const name = decodeURIComponent(this.url.pathname.replace('/original/', ''));
    const filename = originals.get(name);
    assert.ok(filename, 'unknown original ' + name);
    const size = fs.statSync(filename).size;
    const range = this.range ? /^bytes=(\d+)-(\d+)$/.exec(this.range).slice(1).map(Number) : [0, size - 1];
    const [start, end] = range;
    assert.ok(start >= 0 && end < size && end - start < 1024 * 1024);
    const bytes = Buffer.alloc(end - start + 1), fd = fs.openSync(filename, 'r');
    try { assert.equal(fs.readSync(fd, bytes, 0, bytes.length, start), bytes.length); }
    finally { fs.closeSync(fd); }
    this.status = this.range ? 206 : 200;
    this.contentRange = `bytes ${start}-${end}/${size}`;
    this.response = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length);
    requests.push({ name, start, bytes: bytes.length });
  }
};
const { default: init, GameLibrary } = await import(pathToFileURL(path.join(packageDir, 'cyrus-assets-worker.js')));
await init({ module_or_path: fs.readFileSync(path.join(packageDir, 'cyrus-assets-worker_bg.wasm')) });
const manifest = JSON.stringify({ files: [...originals].sort().map(([name, filename]) => ({ path: name, bytes: fs.statSync(filename).size, digest: 'a'.repeat(64) })) });
const game = new GameLibrary('https://example.test/original/', manifest, () => {});
const worlds = JSON.parse(game.worlds());
assert.ok(worlds.includes('START') && worlds.includes('ISLAND'));
let menuClock;
for (const name of ['startup/title.png', 'menu/book.menu', 'hud/overlay.hud', 'sounds/bank.sounds', 'START.scripts']) {
  console.log('Preparing ' + name);
  const result = game.prepare(name);
  const files = new Map(result.files);
  assert.ok(files.has(name), 'missing generated asset ' + name);
  if (name === 'menu/book.menu') {
    menuClock = JSON.parse(new TextDecoder().decode(files.get(name))).clock;
    assert.deepEqual(menuClock, {
      normal_frame_rate: 12, min_frame_rate: 6, max_frame_rate: 300,
      use_smooth_fps: true, use_smooth_divisor: true,
    });
  }
  if (name === 'START.scripts') {
    const scripts = JSON.parse(new TextDecoder().decode(files.get(name)));
    assert.equal(scripts.version, 6);
    assert.deepEqual(scripts.clock, menuClock);
    assert.ok(scripts.content_id.length > 0 && scripts.instances.length > 0);
    const animations = JSON.parse(new TextDecoder().decode(files.get('START.animations')));
    assert.equal(animations.version, 2);
    assert.ok(animations.metadata.actors.some(actor => Object.hasOwn(actor, 'model_load_path')), 'model classification metadata survives export');
    assert.equal(Buffer.from(files.get('START.glb')).subarray(0, 4).toString(), 'glTF');
  }
}
const metadata = JSON.parse(new TextDecoder().decode(new Map(game.prepare('movies/0.movie').files).get('movies/0.movie')));
assert.ok(metadata.frames > 0 && metadata.frame_micros > 0);
const before = requests.length;
const first = game.movie_range(0, 0, 256 * 1024);
assert.equal(Buffer.from(first.bytes).subarray(0, 3).toString(), 'SMK');
assert.ok(first.bytes.length < first.total, 'playback reads before complete movie download');
assert.ok(requests.slice(before).every(request => request.bytes <= 1024 * 1024));
assert.throws(() => game.movie_range(0, first.total + 1, 1), /beyond payload/);
assert.throws(() => game.prepare('../START.scripts'), /path/);
let attempts = 0;
const send = XMLHttpRequest.prototype.send;
XMLHttpRequest.prototype.send = function () { attempts++; throw new Error('Disconnected transport'); };
try {
  assert.throws(() => game.worlds(), error => {
    const message = String(error);
    assert.match(message, /WORLD\.INI.*failed after retry.*Disconnected transport/s);
    assert.ok(!message.includes('at XMLHttpRequest'), 'player error excludes JavaScript stack');
    return true;
  });
  assert.equal(attempts, 2, 'read retries once before reporting its contextual error');
} finally { XMLHttpRequest.prototype.send = send; }
game.free();
console.log('Installed WASM startup/menu/HUD/sound/world exports, script identity, model metadata and bounded CD movie reads passed');
