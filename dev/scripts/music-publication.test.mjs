import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('prepared CD publication allocates nothing and shares the retirement clock', async () => {
  const nodes = [], buffers = [];
  let now = 1;
  globalThis.document = { addEventListener() {} };
  globalThis.AudioContext = class {
    sampleRate = 44100;
    state = 'running';
    destination = {};
    get currentTime() { return now; }
    createBuffer(channels, length, sampleRate) {
      const buffer = { length, sampleRate, copyToChannel() {} };
      buffers.push(buffer);
      return buffer;
    }
    createGain() { return { gain: {}, connect() {}, disconnect() {} }; }
    createBufferSource() {
      const source = {
        connect() {}, disconnect() { this.disconnected = true; },
        start(when) { this.startAt = when; },
        stop(when) { this.stopAt = when ?? now; },
      };
      nodes.push(source);
      return source;
    }
    async resume() {}
  };
  const code = await readFile(new URL('../../crates/cyrus-audio/src/music/browser.js', import.meta.url));
  const {createMusic, prepareMusic, publishMusic, stopMusic, musicFrames} =
    await import('data:text/javascript;base64,' + code.toString('base64'));
  const pcm = new Float32Array(441000);
  const old = createMusic(pcm, pcm, 44100, 1, false);
  const abandoned = prepareMusic(pcm, pcm, 44100, 1);
  assert.equal(nodes[1].startAt, undefined, 'preparation must stay silent');
  now = 2;
  stopMusic(abandoned);
  assert.equal(nodes[0].stopAt, undefined, 'rejection must retain old output');
  assert.equal(musicFrames(old), 44100, 'old cursor advances during preparation');
  assert.equal(nodes[1].disconnected, true, 'abandoned graph must be released');
  const replacement = prepareMusic(pcm, pcm, 44100, 1);
  const preparedNodes = nodes.length, preparedBuffers = buffers.length;
  now = 3;
  publishMusic(replacement, new Uint32Array([old]), false);
  assert.equal(nodes.length, preparedNodes);
  assert.equal(buffers.length, preparedBuffers);
  assert.equal(nodes[2].startAt, 3);
  assert.equal(nodes[0].stopAt, 3);
  assert.equal(nodes[0].disconnected, true);
  assert.equal(musicFrames(replacement), 0, 'prepared playback must start at its saved cursor origin');
  now = 4;
  assert.equal(musicFrames(replacement), 44100);
  const paused = prepareMusic(pcm, pcm, 44100, 0.5);
  publishMusic(paused, new Uint32Array([replacement]), true);
  assert.equal(nodes[3].startAt, undefined, 'paused checkpoint stays paused');
  assert.equal(nodes[2].stopAt, 4);
  stopMusic(paused);
  delete globalThis.AudioContext;
  delete globalThis.document;
});

test('CD continuation wakes a suspended context after paused preparation', async () => {
  let resumes = 0, starts = 0;
  globalThis.document = { addEventListener() {} };
  globalThis.AudioContext = class {
    sampleRate = 44100;
    state = 'suspended';
    currentTime = 0;
    destination = {};
    createBuffer(channels, length, sampleRate) {
      return { length, sampleRate, copyToChannel() {} };
    }
    createGain() { return { gain: {}, connect() {}, disconnect() {} }; }
    createBufferSource() {
      return { connect() {}, disconnect() {}, stop() {}, start() { starts++; } };
    }
    async resume() { resumes++; this.state = 'running'; }
  };
  try {
    const code = await readFile(new URL('../../crates/cyrus-audio/src/music/browser.js', import.meta.url));
    const {createMusic, updateMusic, stopMusic} = await import(
      'data:text/javascript;base64,' + code.toString('base64') + '#continuation');
    const pcm = new Float32Array(44100);
    const id = createMusic(pcm, pcm, 44100, 1, true);
    assert.equal(starts, 0, 'paused preparation stays silent');
    assert.equal(resumes, 0, 'paused preparation does not wake the context');
    updateMusic(id, 1, false);
    assert.equal(starts, 1, 'continuation starts the retained source');
    assert.equal(resumes, 1, 'continuation must wake the output clock');
    updateMusic(id, 1, false);
    assert.equal(starts, 1, 'ordinary updates cannot duplicate the source');
    assert.equal(resumes, 1, 'ordinary updates do not repeat context resume');
    stopMusic(id);
  } finally {
    delete globalThis.AudioContext;
    delete globalThis.document;
  }
});
