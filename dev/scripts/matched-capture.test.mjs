import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkView } from './matched-capture.mjs';

test('matching quaternion signs describe the same capture orientation', () => {
  checkView([1, 2, 3, 0, 0, 0, 1], [1, 2, 3, 0, 0, 0, -1]);
});
test('an unrelated world camera cannot produce an accepted triplet', () => {
  assert.throws(() => checkView([1, 2, 3, 0, 0, 0, 1], [4, 2, 3, 0, 0, 0, 1]), /position/);
  assert.throws(() => checkView([1, 2, 3, 0, 0, 0, 1], [1, 2, 3, 0, 1, 0, 0]), /orientation/);
  assert.throws(() => checkView([1, 2, 3, 0, 0, 0, 1], null), /absent/);
});

test('an existing capture label fails before invoking any game', async () => {
  const { mkdtempSync, mkdirSync, writeFileSync, rmSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { capture } = await import('./matched-capture.mjs');
  const before = process.cwd();
  const directory = mkdtempSync(`${tmpdir()}/cyrus-capture-test-`);
  try {
    process.chdir(directory);
    writeFileSync('fixture.json', JSON.stringify({version: 1, world: 'TEST', view: [0, 0, 0, 0, 0, 0, 1], viewport: [640, 480], settle_ms: 0}));
    mkdirSync('magic/captures/existing', {recursive: true});
    await assert.rejects(capture('fixture.json', 'existing'), /capture already exists/);
  } finally {
    process.chdir(before);
    rmSync(directory, {recursive: true, force: true});
  }
});
