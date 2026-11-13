import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkView, submitCaptureInstall, captureNativeFrame } from './matched-capture.mjs';

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

test('capture submits the explicitly selected server and rejects a different remembered location', () => {
  let submissions = 0;
  const picker = { requestSubmit() { submissions++; } };
  const input = { value: 'https://example.test/original/' };
  globalThis.location = { href: 'https://example.test/?install=https://example.test/original/' };
  globalThis.document = { getElementById(id) { return id === 'install-picker' ? picker : input; } };
  try {
    assert.equal(submitCaptureInstall('https://example.test/original/'), true);
    assert.equal(submissions, 1);
    input.value = 'https://different.test/original/';
    assert.throws(() => submitCaptureInstall('https://example.test/original/'), /explicitly supplied server/);
    assert.equal(submissions, 1);
    document.getElementById = () => null;
    assert.equal(submitCaptureInstall('https://example.test/original/'), false);
  } finally {
    delete globalThis.document;
    delete globalThis.location;
  }
});

test('native image uses its owning render receipt rather than an earlier polled state', async () => {
  const state = {scene: 'ISLAND', scripts: {ordinary_updates: 10}};
  const boundary = {scene: 'ISLAND', ready: true, execution: 'running', generation: 1,
    event: 'render_frame', phase: 'render_extraction', render_frame: 17, update: 13,
    elapsed_scale: 4608, normal_frame_rate: 12, game_seconds: 1.5};
  const response = {captured: true, path: 'frame.png', boundary};
  const send = async request => {
    assert.deepEqual(request, {type: 'capture_gameplay_frame', path: 'frame.png'});
    return response;
  };
  assert.deepEqual(await captureNativeFrame(state, 1, 'frame.png', send), response);
  assert.equal(response.boundary.update, 13);
  for (const bad of [
    {path: 'frame.png'},
    {...response, path: 'other.png'},
    {...response, boundary: {...boundary, scene: 'START'}},
    {...response, boundary: {...boundary, generation: 2}},
    {...response, boundary: {...boundary, phase: 'polled'}},
    {...response, boundary: {...boundary, elapsed_scale: 4609}},
    {...response, boundary: {...boundary, ready: false}},
    {...response, boundary: {...boundary, update: 9}},
    {...response, boundary: {...boundary, render_frame: 0}},
  ]) await assert.rejects(captureNativeFrame(state, 1, 'frame.png', async () => bad));
});
