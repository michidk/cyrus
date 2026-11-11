// Installed, sealed-browser acceptance for diagnostic session publication (#46).
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { observeBrowserBuild } from './browser-provenance.mjs';
import { connectCdp } from './capture-cdp.mjs';

const [endpoint, page, install, scene = '20'] = process.argv.slice(2);
assert(endpoint && page && install, 'usage: node dev/scripts/test-browser-session-cd.mjs CDP_URL PAGE_URL INSTALL_URL [SCENE]');
const url = new URL(page);
url.search = new URLSearchParams({ scene, capture: '1', install: new URL(install).href });
const build = JSON.parse(readFileSync('target/web/build-provenance.json'));
const cdp = await connectCdp(endpoint);
let provenance, id = 0, lastFrame, progressedAt = performance.now();
const ceil = frame => Math.ceil(frame / 128) * 128;
const floor = frame => Math.floor(frame / 128) * 128;

async function evaluate(expression) {
  const { result, exceptionDetails } = await cdp.call('Runtime.evaluate', {
    expression, returnByValue: true, awaitPromise: true,
  });
  if (exceptionDetails) throw Error(result.description ?? exceptionDetails.text);
  return result.value;
}

async function until(label, condition, limit = 30000, watchGame = true) {
  const deadline = performance.now() + limit;
  while (performance.now() < deadline) {
    const value = await condition();
    if (value) return value;
    if (watchGame) {
      const frame = await evaluate('window.__cyrusCapture?.scripts?.globals?.world_loop_frame');
      if (frame !== lastFrame) {
        lastFrame = frame;
        progressedAt = performance.now();
      }
      assert(performance.now() - progressedAt < 20000, `${label}: game clock stalled at ${frame}`);
    }
    await delay(100);
  }
  throw Error(`Timed out: ${label}`);
}

async function request(type, fields = {}) {
  const current = ++id;
  await evaluate(`window.__cyrusSessionRequest = ${JSON.stringify(JSON.stringify({ type, id: current, ...fields }))}; true`);
  return until(type, () => evaluate(`window.__cyrusSessionResult?.id === ${current} ? window.__cyrusSessionResult : null`));
}

async function operation(type, fields) {
  const deadline = performance.now() + 55000;
  let result = await request(type, fields);
  while (result.pending) {
    assert(performance.now() < deadline, `${type}: publication deadline`);
    await delay(100);
    result = await request('result');
  }
  return result;
}

function documentBody(document) {
  const [header, hash, ...body] = document.split('\n');
  assert.equal(header, 'CYRUS-SESSION 1');
  assert.match(hash, /^[0-9a-f]{64}$/);
  return JSON.parse(body.join('\n'));
}

async function store(key, document) {
  return evaluate(`(${async (key, document) => {
    const db = await new Promise((resolve, reject) => {
      const request = indexedDB.open('cyrus-sessions', 1);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    try {
      return await new Promise((resolve, reject) => {
        const transaction = db.transaction('documents', document === undefined ? 'readonly' : 'readwrite');
        const store = transaction.objectStore('documents');
        const request = document === undefined ? store.get(key) : store.put(document, key);
        transaction.oncomplete = () => resolve(request.result);
        transaction.onabort = () => reject(transaction.error);
      });
    } finally {
      db.close();
    }
  }})(${JSON.stringify(key)}, ${JSON.stringify(document)})`);
}

async function graph() {
  const state = await evaluate('window.__sessionCd.status()');
  assert.equal(state.error, null);
  return state;
}

async function owner() {
  const result = await operation('capture');
  assert(!result.error, result.error);
  const body = documentBody(result.document);
  const registry = body.logical.registry;
  assert(!registry.error, registry.error);
  return {
    world: registry.current,
    generation: registry.worlds.find(w => w.world === registry.current).generation,
    track: body.music.track,
  };
}

async function match(index, from, to, expected, offset) {
  await until('rendered quanta', () => evaluate(
    `window.__sessionCd.blocks.some(b => b.frame >= ${to - 128})`,
  ));
  return evaluate(`window.__sessionCd.match(${[index, from, to, expected, offset].join(',')})`);
}

try {
  provenance = await observeBrowserBuild(cdp, build, url);
  await cdp.call('Page.enable');
  await cdp.call('Page.addScriptToEvaluateOnNewDocument', {
    source: readFileSync(new URL('./session-cd-graph.js', import.meta.url), 'utf8'),
  });
  await cdp.call('Page.navigate', { url: url.href });
  await until('secure browser page', () => evaluate('document.readyState === "complete" && !!document.body'), 30000, false);
  assert(await evaluate('isSecureContext && !!AudioWorkletNode'), 'use a secure origin for WebAudio and install access');
  await until('installed gameplay', () => evaluate(`(() => {
    const picker = document.getElementById('install-picker');
    if (picker && document.getElementById('install-url')?.value && !picker.dataset.sessionCdSubmitted) {
      picker.dataset.sessionCdSubmitted = 'true';
      picker.requestSubmit();
    }
    window.__cyrusInspectScripts = true;
    return window.__cyrusCapture?.ready && window.__cyrusCapture?.scripts?.globals?.world_loop_frame > 10;
  })()`), 210000, false);
  progressedAt = performance.now();
  const point = await evaluate(`(() => {
    const r = document.querySelector('canvas').getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  })()`);
  for (const type of ['mousePressed', 'mouseReleased']) {
    await cdp.call('Input.dispatchMouseEvent', { type, button: 'left', clickCount: 1, ...point });
  }
  const first = await until('initial CD graph', async () => {
    const state = await graph();
    return state.ready && state.sources[0]?.now > state.sources[0]?.frame + 8192 && state.sources[0];
  });
  assert.equal(first.offset, 0);
  assert.equal(first.rate, first.bufferRate, 'sample oracle requires the installed CD rate');
  const initial = await until('capturable scene', async () => {
    try { return await owner(); }
    catch (error) {
      if (!String(error).includes('displayed model mesh has not settled')) throw error;
    }
  });
  assert(initial.track > 0, 'fixture has no CD track');
  console.log(JSON.stringify({ initial, trackSeconds: first.length / first.rate }));
  const nextSeam = first.frame + first.length * Math.ceil((first.now - first.frame + 4 * first.rate) / first.length);
  await until('approaching installed track seam', async () =>
    (await graph()).sources[0].now >= nextSeam - 4 * first.rate, 390000);

  const saved = await operation('save', { key: 'cd-valid' });
  assert.equal(saved.stored, true, saved.error);
  const document = await store('cd-valid');
  const body = documentBody(document);
  assert.equal(body.music.track, initial.track);
  const samples = body.music.playback.transport.input_samples;
  assert(Number.isSafeInteger(samples) && samples % 2 === 0 && samples > 0 && samples < first.length * 2);
  const cursor = samples / 2;
  const toSeam = first.length - cursor;
  assert(toSeam > 8192 && toSeam <= first.rate * 5, `saved cursor missed seam window: ${toSeam} frames left`);

  assert(body.hud, 'fixture has no HUD owner to reject during publication');
  // Decode and re-encode in Python so the negative control preserves integers
  // outside JavaScript's exactly representable range.
  const invalid = execFileSync(process.env.SESSION_CD_PYTHON ?? 'python3', ['-c', `
import json, sys, blake3
header, digest, data = sys.stdin.buffer.read().split(b'\\n', 2)
assert blake3.blake3(data).hexdigest().encode() == digest
body = json.loads(data)
body['hud'] = None
data = json.dumps(body, separators=(',', ':')).encode()
sys.stdout.buffer.write(b'\\n'.join((header, blake3.blake3(data).hexdigest().encode(), data)))
`], { input: document, encoding: 'utf8', maxBuffer: 2 * Buffer.byteLength(document) });
  await store('cd-reject', invalid);
  const before = (await graph()).sources[0].now;
  const rejection = await operation('load', { key: 'cd-reject' });
  assert.match(rejection.error ?? '', /session publication HUD host census differs/);
  assert.deepEqual(await owner(), initial, 'rejected publication changed live world, generation or track');
  const rejected = await graph();
  assert.equal(rejected.sources.length, 1, 'rejected publication started another CD source');
  assert.equal(rejected.sources[0].stopped, null);
  assert.equal(rejected.sources[0].disconnected, null);
  const rejectedPcm = await match(0, floor(before) - 4096, ceil(rejected.sources[0].now) + 8192, first.frame, 0);
  console.log(JSON.stringify({ rejection: rejection.error, rejectedPcm }));

  const committed = await operation('load', { key: 'cd-valid' });
  assert.equal(committed.committed, true, committed.error);
  const after = await until('replacement CD graph', async () => {
    const state = await graph();
    return state.sources.length >= 2 && state;
  });
  assert.equal(after.sources.length, 2);
  assert(after.sameClock, 'CD sources cannot be compared on different render clocks');
  const [old, restored] = after.sources;
  assert(old.stopped !== null && old.disconnected !== null, 'published session retained the old source');
  assert(old.stopped <= restored.frame && old.disconnected <= restored.frame, 'old and replacement CD sources overlapped');
  assert.equal(restored.length, old.length);
  assert.equal(restored.rate, old.rate);
  assert.equal(restored.bufferRate, old.bufferRate);
  assert.equal(restored.offset, 0);
  const restoredStart = ceil(restored.frame) + 4096;
  const restoredEnd = ceil(restored.frame + toSeam + restored.rate);
  const restoredPcm = await match(1, restoredStart, restoredEnd, restored.frame, cursor);
  await assert.rejects(match(1, restoredStart, restoredEnd, restored.frame, cursor + 512),
    /rendered CD mismatch/, 'shifted cursor must fail the PCM oracle');
  const oldEnd = floor(old.stopped) - 128;
  const beforeRetirement = await match(0, oldEnd - 44160, oldEnd, first.frame, 0);
  const transition = await evaluate(`window.__sessionCd.transition(${oldEnd}, ${ceil(restored.frame) + 4096},
    ${ceil(old.stopped) + 128}, ${floor(restored.frame) - 128})`);
  assert.equal((await graph()).sources.length, 2, 'CD source was replaced again after publication');
  const { observed } = await provenance.verify();
  console.log(JSON.stringify({ savedCursor: cursor, toSeam, retiredAt: old.stopped, disconnectedAt: old.disconnected,
    restoredAt: restored.frame, beforeRetirement, transition, restoredPcm,
    revision: build.source.revision, sourceHash: build.source.sha256, observed }));
} catch (error) {
  console.error(await evaluate(`({ graph: window.__sessionCd?.status(), page: document.body?.innerText.slice(-1500),
    ready: window.__cyrusCapture?.ready, frame: window.__cyrusCapture?.scripts?.globals?.world_loop_frame })`).catch(() => null));
  throw error;
} finally {
  provenance?.stop();
  await cdp.close();
}
