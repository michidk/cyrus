// Render production browser CD sources in Chromium without a device or game data.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { connectCdp } from './capture-cdp.mjs';

const source = await readFile(new URL('../../crates/cyrus-audio/src/music/browser.js', import.meta.url));
const moduleUrl = `data:text/javascript;base64,${source.toString('base64')}`;

async function renderReplacement(moduleUrl, lateRetirement) {
  const rate = 44100;
  const boundary = 4096;
  const replacement = boundary + 128;
  const length = 16384;
  const saved = 1024;
  const context = new OfflineAudioContext(2, length, rate);
  // Only context creation/resume are substituted; source nodes, scheduling,
  // connections and every rendered sample use Chromium's audio graph.
  globalThis.AudioContext = class {
    constructor() {
      return new Proxy(context, {
        get(target, name) {
          if (name === 'resume') return async () => {};
          const value = Reflect.get(target, name, target);
          return typeof value === 'function' ? value.bind(target) : value;
        },
      });
    }
  };
  const {createMusic, stopMusic, musicFrames} = await import(moduleUrl);
  const left = Float32Array.from({length: 8192}, (_, i) => (4096 + i) / 32768);
  const right = left.map(x => -x);
  const old = createMusic(left, right, rate, 1, false);
  const publish = context.suspend(boundary / rate);
  const nextUpdate = context.suspend(replacement / rate);
  const output = context.startRendering();
  await publish;
  const oldCursor = musicFrames(old);
  if (!lateRetirement) stopMusic(old);
  await context.resume();
  await nextUpdate;
  if (lateRetirement) stopMusic(old);
  // Rust's CD checkpoint prepares a buffer rotated to the saved stereo frame.
  const rotate = channel => channel.map((_, i) => channel[(i + saved) % channel.length]);
  const restored = createMusic(rotate(left), rotate(right), rate, 1, false);
  const initialCursor = musicFrames(restored);
  await context.resume();
  const pcm = await output;
  stopMusic(restored);
  let preError = 0, restoredError = 0, staleFrames = 0;
  for (const [index, expected] of [left, right].entries()) {
    const channel = pcm.getChannelData(index);
    for (let i = 0; i < boundary; i++) {
      preError = Math.max(preError, Math.abs(channel[i] - expected[i]));
    }
    for (let i = boundary; i < replacement; i++) {
      if (index === 0 && channel[i] !== 0) staleFrames++;
    }
    for (let i = replacement; i < length; i++) {
      const frame = (saved + i - replacement) % expected.length;
      restoredError = Math.max(restoredError, Math.abs(channel[i] - expected[frame]));
    }
  }
  return {oldCursor, initialCursor, saved, preError, restoredError, staleFrames,
    restoredFrames: length - replacement};
}

const cdp = await connectCdp(process.argv[2] ?? 'http://127.0.0.1:9227');
try {
  const result = {};
  for (const late of [true, false]) {
    const {result: evaluated, exceptionDetails} = await cdp.call('Runtime.evaluate', {
      expression: `(${renderReplacement})(${JSON.stringify(`${moduleUrl}#${late}`)}, ${late})`,
      awaitPromise: true,
      returnByValue: true,
    });
    assert(!exceptionDetails, evaluated?.description ?? exceptionDetails?.text);
    const sample = evaluated.value;
    assert.equal(sample.oldCursor, 4096);
    assert.equal(sample.initialCursor, 0);
    assert.equal(sample.preError, 0);
    assert.equal(sample.restoredError, 0);
    assert.equal(sample.staleFrames, late ? 128 : 0);
    result[late ? 'lateRetirementControl' : 'publicationRetirement'] = sample;
  }
  console.log(JSON.stringify({sourceSha256: createHash('sha256').update(source).digest('hex'), ...result}));
} finally {
  await cdp.close();
}
