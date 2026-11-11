// Installed public session speech/effect ownership and actual mixed-graph acceptance.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { observeBrowserBuild } from './browser-provenance.mjs';
import { connectCdp } from './capture-cdp.mjs';

const [endpoint, page, install, label, effect = '25', scene = '20'] = process.argv.slice(2);
assert(endpoint && page && install && label, 'usage: node dev/scripts/test-browser-session-samples.mjs CDP_URL PAGE_URL INSTALL_URL VOICE_LABEL [LOOP_EFFECT] [SCENE]');
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
  const state = await evaluate('window.__sessionSamples.status()');
  assert.equal(state.error, null); return state;
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

try {
  provenance = await observeBrowserBuild(cdp, build, url);
  await cdp.call('Page.enable');
  await cdp.call('Page.addScriptToEvaluateOnNewDocument', {
    source: readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8'),
  });
  await cdp.call('Page.navigate', { url: url.href });
  await until('secure browser page', () => evaluate(`location.origin === ${JSON.stringify(url.origin)} && document.readyState === "complete" && !!document.body`), 30000, false);
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
  const capture = async () => {
    const result = await operation('capture');
    assert(!result.error, result.error); return documentBody(result.document);
  };
  const initial = await until('capturable audio world', async () => {
    try { return await owner(); } catch { return false; }
  });
  const startId = ++id;
  await evaluate(`window.__cyrusAudioRequest = ${JSON.stringify(JSON.stringify({id: startId, generation: initial.generation, index: 0, label, effect: Number(effect)}))}; true`);
  const started = await until('installed speech and loop start', () => evaluate(`window.__cyrusAudioResult?.id === ${startId} && !window.__cyrusAudioResult.pending && window.__cyrusAudioResult`));
  assert.equal(started.started, true, started.error);
  await until('rendering shared sample stream', async () => (await graph()).ready);
  await until('active decoded voice and looping effect', async () => {
    const state = (await capture()).samples;
    return state.players.some(p => p.dialogue_label && p.transport.input_samples > 0)
      && state.ownership.samples.slots.some(s => s?.effect === Number(effect) && s.owner?.Script);
  });
  const saved = await operation('save', {key: 'samples-valid'});
  assert.equal(saved.stored, true, saved.error);
  const document = await store('samples-valid'), body = documentBody(document);
  const samples = body.samples;
  const speech = samples.ownership.speech;
  assert(Number.isInteger(speech));
  const voiceSlot = samples.ownership.samples.slots[speech];
  const voice = samples.players.find(p => p.token === voiceSlot.token);
  assert.deepEqual(voice.dialogue_label, [...Buffer.from(label.toLowerCase())]);
  assert(voice.transport.input_samples > 0);
  const loopSlot = samples.ownership.samples.slots.find(s => s?.effect === Number(effect) && s.owner?.Script);
  assert.equal(loopSlot.owner.Script.generation, initial.generation);
  const loop = samples.players.find(p => p.token === loopSlot.token);
  assert.equal(loop.transport.repetitions, 0);
  const pcmWindow = async at => {
    const buffer = await until('nonzero submitted sample output', async () => {
      const state = await graph();
      return state.buffers.find(b => b.frame >= ceil(at) + 4096 && b.energy > 0.001);
    });
    const from = ceil(buffer.frame) + 128, to = from + 8192;
    await until('stereo sample graph window', async () => (await graph()).last >= to);
    return evaluate(`window.__sessionSamples.match(${from}, ${to})`);
  };
  const initialGraph = await pcmWindow((await graph()).now);
  const wrongStart = ceil((await graph()).now) + 4096;
  await until('oracle negative control window', async () => (await graph()).last >= wrongStart + 8192);
  await assert.rejects(evaluate(`window.__sessionSamples.match(${wrongStart}, ${wrongStart + 8192}, 512)`), /sample graph mismatch/);

  // Removing only the active voice's source hint reproduces the old host gap.
  const missing = execFileSync(process.env.SESSION_CD_PYTHON ?? 'python3', ['-c', `
import json, sys, blake3
header, digest, data = sys.stdin.buffer.read().split(b'\\n', 2)
body = json.loads(data)
for player in body['samples']['players']: player.pop('dialogue_label', None)
data = json.dumps(body, separators=(',', ':')).encode()
sys.stdout.buffer.write(b'\\n'.join((header, blake3.blake3(data).hexdigest().encode(), data)))
`], {input: document, encoding: 'utf8', maxBuffer: 2*Buffer.byteLength(document)});
  await store('samples-missing-source', missing);
  const missingSource = await operation('load', {key: 'samples-missing-source'});
  assert.match(missingSource.error ?? '', /no matching installed effect or retained dialogue source/);
  assert.deepEqual(await owner(), initial);
  const invalid = execFileSync(process.env.SESSION_CD_PYTHON ?? 'python3', ['-c', `
import json, sys, blake3
header, digest, data = sys.stdin.buffer.read().split(b'\\n', 2)
assert blake3.blake3(data).hexdigest().encode() == digest
body = json.loads(data)
body['hud'] = None
data = json.dumps(body, separators=(',', ':')).encode()
sys.stdout.buffer.write(b'\\n'.join((header, blake3.blake3(data).hexdigest().encode(), data)))
`], {input: document, encoding: 'utf8', maxBuffer: 2*Buffer.byteLength(document)});
  await store('samples-reject', invalid);
  const rejection = await operation('load', {key: 'samples-reject'});
  assert.match(rejection.error ?? '', /session publication HUD host census differs/);
  assert.deepEqual(await owner(), initial);
  const rejected = (await capture()).samples;
  assert.equal(rejected.ownership.speech, speech);
  const stillVoice = rejected.players.find(p => p.token === voice.token);
  assert(stillVoice, 'rejection lost active voice');
  assert(stillVoice.transport.input_samples > voice.transport.input_samples);
  assert(rejected.players.some(p => p.token === loop.token), 'rejection replaced looping transport');
  const rejectedGraph = await pcmWindow((await graph()).now);
  console.log(JSON.stringify({initial, voiceSlot: speech, savedVoice: voice.transport, savedLoop: loop.transport, initialGraph, missingSource: missingSource.error, rejection: rejection.error, rejectedGraph}));
  const committed = await operation('load', {key: 'samples-valid'});
  assert.equal(committed.committed, true, committed.error);
  const published = await capture(), restored = published.samples;
  const generation = published.logical.registry.worlds.find(w => w.world === published.logical.registry.current).generation;
  assert.equal(restored.ownership.speech, speech, 'restoration lost the voice stop binding');
  const restoredSlot = restored.ownership.samples.slots[speech];
  assert(restoredSlot.token > Math.max(...rejected.players.map(p => p.token)));
  const restoredVoice = restored.players.find(p => p.token === restoredSlot.token);
  assert.equal(restoredVoice.transport.source_identity, voice.transport.source_identity);
  assert(restoredVoice.transport.input_samples >= voice.transport.input_samples);
  assert.equal(restoredVoice.transport.repetitions, 1);
  const restoredLoopSlot = restored.ownership.samples.slots.find(s => s?.effect === Number(effect) && s.owner?.Script);
  assert.equal(restoredLoopSlot.owner.Script.generation, generation);
  assert(restoredLoopSlot.token > loop.token);
  const restoredLoop = restored.players.find(p => p.token === restoredLoopSlot.token);
  assert.equal(restoredLoop.transport.source_identity, loop.transport.source_identity);
  assert.equal(restoredLoop.transport.repetitions, 0);
  assert(restored.players.every(p => !rejected.players.some(old => old.token === p.token)),
    'publication retained an old sample transport');
  const publishedGraph = await pcmWindow((await graph()).now);
  const audible = (await capture()).samples;
  assert(audible.players.find(p => p.token === restoredVoice.token)?.transport.input_samples > restoredVoice.transport.input_samples,
    'restored voice decoder did not advance during actual output');
  const stopId = ++id;
  await evaluate(`window.__cyrusVoiceStopRequest = ${stopId}; true`);
  await until('public examine voice stop', () => evaluate(`window.__cyrusVoiceStopResult === ${stopId}`));
  const stopped = await until('voice retires while loop continues', async () => {
    const value = (await capture()).samples;
    return value.ownership.speech === null && !value.players.some(p => p.token === restoredVoice.token) && value;
  });
  assert(stopped.players.some(p => p.token === restoredLoop.token), 'voice stop retired looping effect');
  // Keep queued mixed buffers intact. Observe new output only beyond their watermark.
  const queued = await graph();
  const drainEnd = Math.max(queued.now, ...queued.buffers.map(b => b.frame+b.length));
  const afterDrainGraph = await pcmWindow(drainEnd);
  const {observed} = await provenance.verify();
  console.log(JSON.stringify({generation, restoredVoice: restoredVoice.transport, restoredVoiceToken: restoredVoice.token,
    restoredLoopToken: restoredLoop.token, publishedGraph, drainEnd, afterDrainGraph, revision: build.source.revision, sourceHash: build.source.sha256, observed}));
} catch (error) {
  console.error(await evaluate(`({audio: window.__cyrusAudioResult, graph: window.__sessionSamples?.status(), frame: window.__cyrusCapture?.scripts?.globals?.world_loop_frame})`).catch(() => null));
  throw error;
} finally {
  provenance?.stop(); await cdp.close();
}
