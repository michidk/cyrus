#!/usr/bin/env node
// Matched captures validate live poses; original instrumentation stays under magic/.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync, existsSync, realpathSync } from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { nativeProvenance } from './capture-provenance.mjs';
import { connectCdp } from './capture-cdp.mjs';
import { observeBrowserBuild } from './browser-provenance.mjs';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
export function fixtureAt(filename) {
  const f = JSON.parse(readFileSync(filename, 'utf8'));
  assert.equal(f.version, 1, 'fixture version');
  assert.match(f.world, /^[A-Z0-9_]+$/, 'world');
  assert.equal(f.view.length, 7, 'view must contain position and quaternion');
  assert(f.view.every(Number.isFinite), 'finite view');
  assert(Math.abs(Math.hypot(...f.view.slice(3)) - 1) < 0.001, 'unit quaternion');
  assert(f.viewport.every(n => Number.isInteger(n) && n > 0), 'viewport');
  assert.equal(f.viewport.length, 2);
  assert(Number.isFinite(f.settle_ms) && f.settle_ms >= 0, 'settle_ms');
  return f;
}
export function checkView(expected, actual) {
  assert(actual?.length === 7 && actual.every(Number.isFinite), 'live camera pose absent');
  assert(expected.slice(0, 3).every((v, i) => Math.abs(v - actual[i]) < 0.01), 'camera position differs from fixture');
  const dot = expected.slice(3).reduce((sum, v, i) => sum + v * actual[i + 3], 0);
  assert(Math.abs(Math.abs(dot) - 1) < 0.00001, 'camera orientation differs from fixture');
}
function run(program, args) {
  const r = spawnSync(program, args, { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(`${program}: ${r.error || r.stderr || r.stdout}`);
  return r.stdout.trim();
}
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
function command(cmd) {
  const address = process.env.CYRUS_CONTROL_ADDR || '127.0.0.1:7117';
  const colon = address.lastIndexOf(':');
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: address.slice(0, colon), port: Number(address.slice(colon + 1)) });
    let data = '';
    socket.setTimeout(30000, () => socket.destroy(new Error('control timeout')));
    socket.on('error', reject);
    socket.on('connect', () => socket.write(JSON.stringify({ id: 1, cmd }) + '\n'));
    socket.on('data', chunk => {
      data += chunk;
      if (!data.includes('\n')) return;
      socket.end();
      try {
        const result = JSON.parse(data.split('\n')[0]);
        assert(result.ok, JSON.stringify(result));
        resolve(result.result);
      } catch (error) { reject(error); }
    });
  });
}
async function until(check, description, timeout = 300000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const value = await check();
    if (value) return value;
    await sleep(250);
  }
  throw new Error(`timed out waiting for ${description}`);
}
async function browser(f, output, build) {
  assert(process.env.CYRUS_CAPTURE_WEB_URL, 'set CYRUS_CAPTURE_WEB_URL to the game server URL reachable by the capture browser');
  assert(process.env.CYRUS_CAPTURE_CDP, 'set CYRUS_CAPTURE_CDP to a Chromium DevTools HTTP endpoint');
  const url = new URL(process.env.CYRUS_CAPTURE_WEB_URL);
  assert(url.searchParams.has('install'), 'browser URL must explicitly supply ?install=');
  url.searchParams.set('scene', f.world);
  url.searchParams.set('capture', 'true');
  url.searchParams.set('view', f.view.join(','));
  const cdp = await connectCdp(process.env.CYRUS_CAPTURE_CDP);
  let provenance;
  try {
    provenance = await observeBrowserBuild(cdp, build, url.href);
    await cdp.call('Runtime.enable');
    await cdp.call('Log.enable');
    await cdp.call('Emulation.setDeviceMetricsOverride', { width: f.viewport[0], height: f.viewport[1], deviceScaleFactor: 1, mobile: false });
    await cdp.call('Page.navigate', { url: url.href });
    const observed = await until(async () => {
      const r = await cdp.call('Runtime.evaluate', { expression: 'window.__cyrusCapture', returnByValue: true });
      return r.result?.value?.ready === true && r.result.value;
    }, 'browser world loading');
    assert.equal(observed.world.toUpperCase(), f.world, 'browser world');
    checkView(f.view, observed.view);
    await sleep(f.settle_ms);
    const r = await cdp.call('Page.captureScreenshot', { format: 'png' });
    writeFileSync(output, Buffer.from(r.data, 'base64'));
    return { url: url.href, observed, provenance: await provenance.verify(), browser: await cdp.call('Browser.getVersion') };
  } finally {
    writeFileSync(path.join(path.dirname(output), 'browser-log.json'), JSON.stringify(cdp.events, null, 2) + '\n');
    provenance?.stop();
    await cdp.close();
  }
}
async function original(fixture, f, directory) {
  const adapter = process.env.CYRUS_ORIGINAL_CAPTURE;
  assert(adapter, 'set CYRUS_ORIGINAL_CAPTURE to the local original-game capture adapter documented in dev/fixtures/README.md');
  assert(realpathSync(adapter).split(path.sep).includes('magic'), 'original instrumentation must live under magic/');
  run(realpathSync(adapter), [path.resolve(fixture), path.resolve(directory)]);
  const observation = JSON.parse(readFileSync(path.join(directory, 'original.json'), 'utf8'));
  assert.equal(observation.world, f.world, 'original world');
  checkView(f.view, observation.view);
  assert.equal(observation.image_sha256, hash(path.join(directory, 'original.png')), 'original image hash');
  return observation;
}
export async function capture(fixture, label, originalOnly = false) {
  assert.match(label, /^[A-Za-z0-9_-]+$/, 'label must be a filename stem');
  const f = fixtureAt(fixture);
  const directory = path.join('magic', 'captures', label);
  assert(!existsSync(directory), `capture already exists: ${directory}; choose a new label`);
  const nativeBuild = originalOnly ? null : nativeProvenance();
  let webBuild;
  if (!originalOnly) {
    run('python3', ['dev/scripts/build-provenance.py', 'verify', 'target/web/build-provenance.json']);
    webBuild = JSON.parse(readFileSync('target/web/build-provenance.json', 'utf8'));
    assert.deepEqual(webBuild.source, nativeBuild.build.source, 'native and browser builds have different source snapshots');
  }
  mkdirSync(directory, { recursive: true });
  const observation = await original(fixture, f, directory);
  if (originalOnly) return directory;
  const state = await until(async () => {
    const state = await command({ type: 'state' });
    return state.state === 'InGame' && state.scene_ready === true && state.scene && state;
  }, 'native world loading');
  assert.equal(state.scene.toUpperCase(), f.world, 'launch native with just run-native --fixture <fixture>');
  assert(state.flycam.enabled, 'native inspection camera is not enabled');
  checkView(f.view, [...state.flycam.position, ...state.flycam.rotation]);
  await sleep(f.settle_ms);
  const nativeImage = path.join(directory, 'native.png');
  assert(!existsSync(nativeImage), 'native screenshot already exists');
  await command({ type: 'screenshot', path: nativeImage });
  await until(() => {
    if (!existsSync(nativeImage)) return false;
    return readFileSync(nativeImage).subarray(-12).equals(Buffer.from('0000000049454e44ae426082', 'hex'));
  }, 'complete native screenshot');
  const web = await browser(f, path.join(directory, 'browser.png'), webBuild);
  assert.deepEqual(nativeProvenance(), nativeBuild, 'native build or container changed during capture');
  for (const name of ['original', 'native', 'browser']) {
    const size = run('identify', ['-format', '%w %h', path.join(directory, `${name}.png`)]);
    assert.equal(size, f.viewport.join(' '), `${name} viewport differs`);
  }
  writeFileSync(path.join(directory, 'manifest.json'), JSON.stringify({
    fixture: path.relative(process.cwd(), fixture), fixture_sha256: hash(fixture),
    revision: run('git', ['rev-parse', 'HEAD']), original: observation, native: state, browser: web,
    native_provenance: nativeBuild,
    images: Object.fromEntries(['original', 'native', 'browser'].map(name => [name, hash(path.join(directory, `${name}.png`))])),
    scope: 'Verified scene/viewpoint capture. Animation phases and gameplay timing are not synchronized.',
  }, null, 2) + '\n');
  return directory;
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    const [fixture, label, mode] = process.argv.slice(2);
    if (mode === '--launch-args') {
      const f = fixtureAt(fixture);
      console.log(`--scene\n${f.world}\n--view=${f.view.join(',')}`);
    } else console.log(await capture(fixture, label, mode === '--original-only'));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
