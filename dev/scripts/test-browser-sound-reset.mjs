import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const source = readFileSync(new URL('./browser-sound-reset.js', import.meta.url), 'utf8');
function probe(value, missing = false) {
  let observations = 0;
  const context = vm.createContext({performance, setTimeout,
    __sessionSamples: {
      status: () => ({ready: true, now: 0, buffers: observations++ ? [{frame: 0, length: 2048, energy: 1}] : [], last: 8320}),
      blocks: Array.from({length: missing ? 63 : 64}, (_, i) => ({frame: 128 + i*128,
        left: new Float32Array(128).fill(value), right: new Float32Array(128).fill(-value)})),
    }});
  vm.runInContext(source, context);
  return context.__cyrusSoundReset;
}
assert((await probe(.1).sample('active', true)).energy > 0);
assert.equal((await probe(0).sample('stopped', false)).energy, 0);
await assert.rejects(probe(.1).sample('stale after stop', false), /cancellation mismatch/);
await assert.rejects(probe(0).sample('missing continuation', true), /cancellation mismatch/);
await assert.rejects(probe(.1, true).sample('missing quantum', true), /missing or duplicate/);
console.log('browser sound reset: five output-oracle controls passed');
