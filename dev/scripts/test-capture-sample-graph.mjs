import assert from 'node:assert/strict';
import {existsSync, mkdtempSync, readFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {captureSampleGraph} from './capture-sample-graph.mjs';

for (const rejected of [false, true]) {
  test(`graph export ${rejected ? 'retains rejected metadata without PCM' : 'preserves exact admitted float32 bytes'}`, async () => {
    const dir = mkdtempSync(join(tmpdir(), 'cyrus-graph-'));
    const output = join(dir, 'recording');
    const samples = Float32Array.from({length: 256}, (_, i) => i / 256 - 0.5);
    let calls = 0;
    const cdp = {async call(method, args) {
      assert.equal(method, 'Runtime.evaluate');
      calls++;
      return {result: {value: args.expression.includes('__sessionSamples.capture(')
        ? {start: 128, end: 256, rate: 48000, quanta: [{frame: 128, sequence: 0}], error: rejected ? 'missing quantum' : null}
        : Buffer.from(samples.buffer).toString('base64')}};
    }};
    try {
      if (rejected) {
        await assert.rejects(captureSampleGraph(cdp, output, 128, 256), /missing quantum/);
        assert.equal(calls, 1, 'rejected PCM must never be read');
        assert(!existsSync(output + '.f32le'));
      } else {
        await captureSampleGraph(cdp, output, 128, 256);
        assert.deepEqual(readFileSync(output + '.f32le'), Buffer.from(samples.buffer));
      }
      const receipt = JSON.parse(readFileSync(output + '.json'));
      assert.equal(receipt.quanta[0].sequence, 0);
      assert.equal(receipt.exported, !rejected);
      if (!rejected) assert.equal(receipt.pcm.bytes, 1024);
      await assert.rejects(captureSampleGraph(cdp, output, 128, 256), /already exists/);
    } finally { rmSync(dir, {recursive: true, force: true}); }
  });
}

test('endpoint freezes source lifecycle and PCM before asynchronous readback', async () => {
  const {runInNewContext} = await import('node:vm');
  const {captureSampleGraphAtEndpoint} = await import('./capture-sample-graph.mjs');
  const dir = mkdtempSync(join(tmpdir(), 'cyrus-endpoint-'));
  const output = join(dir, 'recording');
  const source = {when: 1, stop: null};
  const pcm = Float32Array.from({length: 256}, () => 0.125);
  const probe = {
    blocks: [{frame: 128}],
    capture(start, end) {
      this.capturedPcm = pcm.slice();
      return this.lastCapture = {start, end, rate: 48000};
    },
  };
  const sandbox = {__sessionSamples: probe, sources: [source], structuredClone, Uint8Array,
    btoa: value => Buffer.from(value, 'binary').toString('base64')};
  let reads = 0;
  const cdp = {async call(_method, {expression}) {
    const value = runInNewContext(expression, sandbox);
    if (++reads === 1) {
      // Playback and ancillary observation continue after the endpoint task.
      source.stop = 2;
      sandbox.sources.push({when: 2, stop: null});
      probe.blocks.push({frame: 256});
      pcm.fill(0.5);
    }
    return {result: {value}};
  }};
  try {
    const receipt = await captureSampleGraphAtEndpoint(cdp, output, '({sources})');
    assert.equal(receipt.start, 128);
    assert.equal(receipt.end, 256);
    assert.deepEqual(receipt.observation.sources, [{when: 1, stop: null}]);
    assert.deepEqual(readFileSync(output + '.f32le'), Buffer.from(Float32Array.from({length: 256}, () => 0.125).buffer));
    assert.equal(sandbox.sources.length, 2, 'control must continue playback during readback');
    assert.notDeepEqual(structuredClone({sources: sandbox.sources}), receipt.observation,
      'a deferred lifecycle snapshot must fail endpoint equivalence');
    assert.notEqual(probe.blocks.at(-1).frame + 128, receipt.end,
      'deferred window selection must include unwanted later playback');
  } finally { rmSync(dir, {recursive: true, force: true}); }
});
