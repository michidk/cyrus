import assert from 'node:assert/strict';
import { openGameAssets } from '../web/game-assets.js';

const calls = [];
let worker;
const assets = await openGameAssets('https://example.test/original/', { createWorker() {
  worker = { postMessage(data) {
    calls.push(data);
    queueMicrotask(() => {
      if (data.kind === 'game-open') this.onmessage({ data: { id: data.id, worlds: '[["2","START"]]' } });
      if (data.kind === 'game-prepare') {
        const key = data.path.replace(/\.(dialogue|wav)$/, '');
        const files = data.path.startsWith('dialogue/')
          ? [[key + '.dialogue', new Uint8Array([3])], [key + '.wav', new Uint8Array([4])]]
          : [['START.glb', new Uint8Array([1])], ['START.scripts', new Uint8Array([2])]];
        this.onmessage({ data: { id: data.id, files } });
      }
      if (data.kind === 'movie-range') this.onmessage({ data: { id: data.id, bytes: new Uint8Array(Math.min(256 * 1024, 512 * 1024 - data.start)), total: 512 * 1024 } });
    });
  }, terminate() { this.terminated = true; } };
  return worker;
} });
assert.deepEqual(assets.worlds, [['2', 'START']]);
assert.deepEqual(await Promise.all([assets.read('START.glb'), assets.read('START.scripts')]), [new Uint8Array([1]), new Uint8Array([2])]);
assert.equal(calls.filter(call => call.kind === 'game-prepare').length, 1, 'world sidecars share one preparation');
const beforeDialogue = calls.length;
assert.deepEqual(await Promise.all([assets.read('dialogue/74657874.dialogue'), assets.read('dialogue/74657874.wav')]), [new Uint8Array([3]), new Uint8Array([4])]);
assert.equal(calls.slice(beforeDialogue).filter(call => call.kind === 'game-prepare').length, 1, 'entry and voice share one preparation');
const manyEntries = Array.from({ length: 25 }, (_, index) => `dialogue/${index.toString(16).padStart(8, '0')}.dialogue`);
assert.deepEqual(await Promise.all(manyEntries.map(path => assets.read(path))), manyEntries.map(() => new Uint8Array([3])), 'in-flight reads survive staging eviction');
const beforeReload = calls.length;
await assets.read('dialogue/74657874.dialogue');
assert.equal(calls.length, beforeReload + 1, 'old staging payload is discarded');
assert.deepEqual(await assets.read('START.glb'), new Uint8Array([1]), 'dialogue retention leaves current world intact');
const controller = new AbortController();
const reader = assets.response('/movies/0.smk', { signal: controller.signal }).body.getReader();
assert.equal((await reader.read()).value.length, 256 * 1024);
controller.abort(new Error('Cancelled movie'));
await assert.rejects(reader.read(), /Cancelled movie/);
const reads = calls.filter(call => call.kind === 'movie-range');
assert.ok(reads.length <= 2 && reads.every(call => call.length === 256 * 1024), 'cancellation permits at most one bounded in-flight read');
worker.onerror({ message: 'Worker stopped' });
await assert.rejects(assets.read('missing.glb'), /Worker stopped/);
assets.close();
assert.equal(worker.terminated, true);
console.log('game asset transport: group deduplication, bounded ranges, cancellation and terminal worker errors passed');
