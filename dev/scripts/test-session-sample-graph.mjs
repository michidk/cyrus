// Observer failures must reject evidence without changing production playback.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';

const script = readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8');
function observer(context, connect = () => {}) {
  let starts = 0;
  class AudioNode { connect(...args) { return connect(...args); } }
  class AudioBufferSourceNode extends AudioNode {
    constructor() { super(); this.context = context; this.buffer = {numberOfChannels: 2, getChannelData: () => new Float32Array(128)}; }
    start() { starts++; return 'started'; }
  }
  const scope = {AudioNode, AudioBufferSourceNode, AudioWorkletNode: AudioNode,
    URL: {createObjectURL: () => 'blob:probe'}, Blob, Float32Array};
  vm.runInNewContext(script, scope);
  return {scope, source: new AudioBufferSourceNode(), starts: () => starts};
}

test('missing AudioWorklet rejects capture and preserves source start', () => {
  const p = observer({currentTime: 0, sampleRate: 44100});
  assert.equal(p.source.start(), 'started');
  assert.equal(p.starts(), 1);
  assert.match(p.scope.__sessionSamples.error, /AudioWorklet/);
  assert.equal(p.scope.__sessionSamples.status().ready, false);
  assert.throws(() => p.scope.__sessionSamples.match(0, 0), /sample graph rejected:.*AudioWorklet/);
});

test('synchronous observer setup failure does not escape into the mixer', () => {
  const p = observer({audioWorklet: {addModule() { throw Error('probe setup failed'); }}});
  assert.equal(p.source.start(), 'started');
  assert.equal(p.starts(), 1);
  assert.match(p.scope.__sessionSamples.error, /probe setup failed/);
});

test('asynchronous observer setup failure rejects capture', async () => {
  const p = observer({audioWorklet: {addModule: () => Promise.reject(Error('module rejected'))}});
  assert.equal(p.source.start(), 'started');
  await new Promise(resolve => setImmediate(resolve));
  assert.match(p.scope.__sessionSamples.error, /module rejected/);
  assert.equal(p.starts(), 1);
});

test('a rejected capture stays rejected while subsequent production buffers start', () => {
  const p = observer({currentTime: 0, sampleRate: 44100});
  p.source.start();
  const error = p.scope.__sessionSamples.error;
  p.source.context = {currentTime: 1, sampleRate: 48000};
  assert.equal(p.source.start(), 'started');
  assert.equal(p.starts(), 2);
  assert.equal(p.scope.__sessionSamples.error, error);
  assert.equal(p.scope.__sessionSamples.blocks.length, 0);
});

test('production start failures retain their exception and are not retried', () => {
  const p = observer({});
  // Install a fresh observer around a failing real start, as a WebAudio invalid-state control.
  p.scope.AudioBufferSourceNode.prototype.start = () => { throw Error('real start rejected'); };
  vm.runInNewContext(script, p.scope);
  assert.throws(() => p.source.start(), /real start rejected/);
  assert.equal(p.scope.__sessionSamples.error, null);
});
