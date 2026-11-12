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
