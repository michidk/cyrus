// A clock sample must not traverse retained PCM or depend on observer health.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
class Source {start() {}}
class Node {connect() {}}
const context = vm.createContext({AudioBufferSourceNode: Source, AudioNode: Node,
  URL: {createObjectURL: () => 'test-worklet'}, Blob: class {}});
vm.runInContext(readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8'), context);
context.__sessionSamples.buffers = new Proxy([], {get() {throw Error('PCM traversed');}});
const source = new Source();
source.buffer = {numberOfChannels: 2};
source.context = {sampleRate: 48000, currentTime: 1.25};
source.start();
assert.match(context.__sessionSamples.error, /AudioWorklet unavailable/);
assert.equal(context.__sessionSamples.clock().now, 60000);
assert.equal(context.__sessionSamples.clock().rate, 48000);
context.__sessionSamples.error = 'capture failure';
assert.equal(context.__sessionSamples.clock().now, 60000);
assert.throws(() => context.__sessionSamples.status(), /PCM traversed/);
console.log('constant-work clock remains independent of PCM traversal and capture failure');
