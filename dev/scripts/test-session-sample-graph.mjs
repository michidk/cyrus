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

function processor(options) {
  let Processor;
  const scope = {AudioWorkletProcessor: class { constructor() { this.port = {postMessage: value => this.messages.push(value)}; this.messages = []; } },
    registerProcessor: (_, value) => { Processor = value; }, currentFrame: 0, console: {timeStamp() {}}, Float32Array};
  const body = script.match(/registerProcessor\('session-sample-tap',[\s\S]*?\n    \}\);/)[0];
  vm.runInNewContext(body, scope);
  const node = new Processor(options);
  const render = (frame, reported = frame, recording = 1, alter = () => {}) => {
    scope.currentFrame = reported;
    const input = [new Float32Array(128), new Float32Array(128)];
    input[0].fill(frame / 10000);
    const parameters = {sampleFrame: Float32Array.from({length: 128}, (_, i) => frame + i), coarseFrame: Float32Array.of(frame), recording: Float32Array.of(recording)};
    alter(parameters, input);
    node.process([input], [], parameters);
    return node.messages.at(-1);
  };
  return {node, render};
}

test('automated sample boundaries survive repeated worklet-global clock values', () => {
  const {node, render} = processor();
  render(0, 0, 0);
  assert.equal(node.messages.length, 0, 'setup lies outside the explicitly armed capture');
  render(128, 128); render(256, 128); render(384, 384);
  assert.deepEqual(Array.from(node.messages, b => b.frame), [128, 256, 384]);
  assert.deepEqual(Array.from(node.messages, b => b.sequence), [0, 1, 2]);
  assert.deepEqual(Array.from(node.messages, b => b.reportedFrame), [128, 128, 384]);
  assert(node.messages.every(b => b.error === null));
  assert.equal(node.messages[1].left[0], Math.fround(256 / 10000));
});

test('automated clock overflow is rejected instead of inventing sample positions', () => {
  const {render} = processor();
  assert.match(render(2 ** 24).error, /invalid automated sample clock/);
});

async function receiver() {
  let tap;
  class AudioNode { connect() {} }
  class AudioBufferSourceNode extends AudioNode {
    constructor() { super(); this.context = context; this.buffer = {numberOfChannels: 2, getChannelData: () => new Float32Array(128)}; }
    start() {}
  }
  class AudioWorkletNode extends AudioNode {
    constructor() { super(); tap = this; this.port = {}; this.parameters = new Map(['sampleFrame', 'coarseFrame', 'recording'].map(name =>
      [name, {setValueAtTime() {}, linearRampToValueAtTime() {}}])); }
  }
  const context = {currentTime: 0, sampleRate: 48000, destination: {}, audioWorklet: {addModule: async () => {}},
    createConstantSource: () => ({offset: {}, start() {}})};
  const scope = {AudioNode, AudioBufferSourceNode, AudioWorkletNode, URL: {createObjectURL: () => 'blob:probe'}, Blob, Float32Array};
  vm.runInNewContext(script, scope);
  new AudioBufferSourceNode().start();
  await new Promise(resolve => setImmediate(resolve));
  const probe = scope.__sessionSamples;
  const send = (frame, sequence) => tap.port.onmessage({data: {frame, sequence, left: new Float32Array(128), right: new Float32Array(128)}});
  return {probe, send};
}

for (const [name, frameDelta, sequence] of [['missing', 256, 2], ['duplicate', 0, 0], ['message loss', 128, 2]]) {
  test(`${name} render quantum permanently rejects capture and retains diagnostics`, async () => {
    const {probe, send} = await receiver();
    const start = probe.startFrame;
    send(start, 0); send(start + frameDelta, sequence); send(start + frameDelta + 128, sequence + 1);
    assert.match(probe.error, /missing or duplicate sample graph quantum/);
    assert.equal(probe.blocks.length, 3);
    assert.throws(() => probe.match(start, start + 128), /sample graph rejected/);
  });
}

test('capture freezes exact samples while retaining diagnostic clock labels', async () => {
  const {probe, send} = await receiver();
  const start = probe.startFrame;
  send(start, 0); send(start + 128, 1);
  probe.blocks[0].left[0] = 0.25;
  const metadata = probe.capture(start, start + 256);
  assert.equal(metadata.quanta.length, 2);
  assert.equal(probe.capturedPcm[0], 0.25);
  probe.blocks[0].left[0] = 0.75;
  send(start + 256, 2);
  assert.equal(metadata.quanta.length, 2);
  assert.equal(probe.capturedPcm[0], 0.25);
  assert.throws(() => probe.capture(start, start + 512), /incomplete sample graph window/);
  assert.equal(probe.capturedPcm, null);
  assert.equal(probe.lastCapture.quanta.length, 3);
});

for (const [name, alter] of [
  ['interior clock hole', p => {p.sampleFrame[40]++;}],
  ['constant clock', p => {p.sampleFrame = Float32Array.of(128);}],
  ['missing channel', (_, input) => {input.pop();}],
]) {
  test(`${name} rejects the rendered quantum`, () => {
    const {render} = processor();
    assert.match(render(128, 128, 1, alter).error, /invalid automated sample clock/);
  });
}

test('triangle rollover preserves absolute identity after a long recording', () => {
  const {render} = processor();
  const base = 131072 * 45;
  const before = render(65536 - 128, 0, 1, p => {p.coarseFrame[0] = base + 65536 - 128;});
  const after = render(0, 0, 1, p => {
    p.coarseFrame[0] = base + 65536;
    p.sampleFrame = Float32Array.from({length: 128}, (_, i) => 65536 - i);
  });
  assert.equal(before.error, null);
  assert.equal(after.error, null);
  assert.equal(after.frame, before.frame + 128);
  assert.equal(after.sequence, before.sequence + 1);
});

test('a slow seek retains the complete armed interval beyond the former rolling window', async () => {
  const {probe, send} = await receiver();
  const start = probe.startFrame;
  for (let i = 0; i < 24001; i++) send(start + i * 128, i);
  assert.equal(probe.error, null);
  assert.equal(probe.blocks.length, 24001);
  assert.equal(probe.blocks[0].frame, start);
  assert.equal(probe.capture(start, start + 24001 * 128).quanta.length, 24001);
});


test('ready observer connection failure rejects evidence without suppressing native activation', async () => {
  const {probe, source, events, failConnection} = await receiverWithOrder();
  events.length = 0; failConnection();
  source.start();
  assert.deepEqual(events, ['connect', 'start']);
  assert.match(probe.error, /connection failed/);
});

test('ready observer connection precedes native source activation', async () => {
  const {probe, source, events} = await receiverWithOrder();
  events.length = 0;
  source.start();
  assert.deepEqual(events.slice(0, 2), ['connect', 'start']);
  assert.equal(probe.error, null);
});

async function receiverWithOrder() {
  const events = [];
  const context = {currentTime: 0, sampleRate: 48000,
    audioWorklet: {addModule: async () => {}}, createConstantSource() { return {offset: {}, start() {}}; }};
  let fail = false;
  class AudioNode { connect() { events.push('connect'); if (fail) throw Error('connection failed'); } }
  class AudioBufferSourceNode extends AudioNode {
    constructor() { super(); this.context = context; this.buffer = {numberOfChannels: 2, getChannelData: () => new Float32Array(128)}; }
    start() { events.push('start'); }
  }
  class AudioWorkletNode extends AudioNode {
    constructor() { super(); this.port = {}; this.parameters = new Map(['sampleFrame', 'coarseFrame', 'recording'].map(name =>
      [name, {setValueAtTime() {}, linearRampToValueAtTime() {}}])); }
  }
  const scope = {AudioNode, AudioBufferSourceNode, AudioWorkletNode,
    URL: {createObjectURL: () => 'blob:probe'}, Blob, Float32Array};
  vm.runInNewContext(script, scope);
  const source = new AudioBufferSourceNode();source.start();
  await new Promise(resolve => setImmediate(resolve));
  return {probe: scope.__sessionSamples, source, events, failConnection: () => { fail = true; }};
}


test('immediate command clocks cannot admit a source consumption model', async () => {
  const {probe, source} = await receiverWithOrder();
  source.start(0);
  const admission = probe.buffers.at(-1).admission;
  assert.equal(admission.kind, 'immediate');
  assert.equal(admission.nativeBefore, 0);
  assert.equal(admission.nativeAfter, 0);
  assert.throws(() => probe.match(0, 128), /immediate source lacks render-generation admission/);
  // Actual graph export remains governed by exact quantum continuity, independently
  // of whether a command-only reconstruction has sufficient source attribution.
  assert.equal(probe.error, null);
});

test('optional clock output supplies the exact generated sample and sequence lanes', () => {
  const {node} = processor();
  const outputs = Array.from({length:2}, () => [new Float32Array(128),new Float32Array(128)]);
  node.process([[new Float32Array(128),new Float32Array(128)]], outputs, {
    sampleFrame:Float32Array.from({length:128},(_,i)=>128+i),
    coarseFrame:Float32Array.of(128),recording:Float32Array.of(1),
  });
  assert.deepEqual(Array.from(outputs[1][0]),Array.from({length:128},(_,i)=>128+i));
  assert(outputs[1][1].every(value=>value===0));
  assert(outputs[0].every(channel=>channel.every(value=>value===0)), 'diagnostic output must remain silent');
});

test('native retirement is acknowledged at the generated quantum, independent of a stale global clock',()=>{
  const {node,render}=processor();
  node.port.onmessage({data:{retire:7}});
  const block=render(256,128);
  assert.equal(block.frame,256);
  assert.deepEqual(Array.from(block.retirements),[7]);
  assert.equal(render(384,128).retirements.length,0);
});

test('ordered clock output uses exact automated sample positions without copying audio or worklet globals',()=>{
 const {node}=processor({processorOptions:{clockOnly:true,startFrame:0}});
 const input=[new Float32Array(128).fill(.25),new Float32Array(128).fill(.5)];
 const outputs=[[new Float32Array(128),new Float32Array(128)],[new Float32Array(128),new Float32Array(128)]];
 const parameters={sampleFrame:Float32Array.from({length:128},(_,i)=>128+i),coarseFrame:Float32Array.of(128),recording:Float32Array.of(1)};
 node.process([input],outputs,parameters);
 assert.deepEqual(Array.from(outputs[1][0]),Array.from({length:128},(_,i)=>128+i));
 assert(outputs[1][1].every(value=>value===1));
 assert(outputs[0].every(channel=>channel.every(value=>value===0)));
 assert.equal(node.messages.length,0,'clock generator cannot impersonate mixed PCM');
 parameters.sampleFrame[10]++;
 node.process([input],outputs,parameters);assert.match(node.messages[0].error,/invalid ordered/);
});
