import assert from 'node:assert/strict';
import * as movie from '../../crates/cyrus-startup/src/movie_stream.js';

let clock = 0;
Object.defineProperty(globalThis, 'performance', { value: { now: () => clock } });
const audios = [];
globalThis.Audio = class {
    constructor(url) {
        this.url = url;
        this.readyState = 0;
        this.seeking = false;
        this.currentTime = 0;
        this.paused = true;
        this.playCount = 0;
        audios.push(this);
    }
    play() { this.paused = false; this.playCount++; return Promise.resolve(); }
    pause() { this.paused = true; }
    removeAttribute() { this.removed = true; }
    load() { this.loaded = true; }
};
let reads = 0, aborted = 0;
globalThis.fetch = async (_, { signal }) => {
    signal.addEventListener('abort', () => { aborted++; });
    return { ok: true, body: { getReader: () => ({ read: async () => {
        reads++;
        return reads <= 10 ? { value: new Uint8Array(1024 * 1024).fill(reads), done: false } : { done: true };
    } }) } };
};
const settle = () => new Promise(resolve => setImmediate(resolve));
const id = movie.openMovie('/cache/0.smk', false);
await settle();
assert.equal(reads, 2, 'fetch must apply backpressure before the complete movie');
assert.equal(movie.movieChunk(id).length, 256 * 1024);
await settle();
assert.equal(reads, 3, 'consuming a chunk must resume the fetch');
assert.equal(movie.movieReady(id), true);
movie.movieVideoEnd(id, 1);
movie.playMovie(id);
clock = 250;
assert.equal(movie.movieTime(id), 0.25);
clock = 2000;
assert.equal(movie.movieTime(id), 1, 'the clock must freeze on video underrun');
movie.movieVideoEnd(id, 2);
clock = 2250;
assert.equal(movie.movieTime(id), 1.25, 'buffering must resume without jumping over missing frames');
movie.seekMovie(id, 5);
movie.movieVideoEnd(id, 6);
movie.playMovie(id);
clock = 2500;
assert.equal(movie.movieTime(id), 5.25);
movie.closeMovie(id);
await settle();
assert.equal(aborted, 1, 'closing a movie must cancel its download');

reads = 0;
const soundId = movie.openMovie('/cache/1.smk', true, 10, 0.1);
const sound = audios.at(-1);
assert.equal(sound.url, '/cache/1.wav');
assert.equal(movie.movieReady(soundId), false, 'wait for the first audio data');
sound.readyState = 2;
assert.equal(movie.movieReady(soundId), true);
movie.movieVideoEnd(soundId, 1);
movie.playMovie(soundId);
await settle();
assert.equal(sound.paused, false);
sound.currentTime = 1;
assert.equal(movie.movieTime(soundId), 1);
assert.equal(sound.paused, true, 'pause audio when video data ends');
movie.movieVideoEnd(soundId, 2);
await settle();
assert.equal(sound.paused, false);
movie.seekMovie(soundId, 10);
assert.equal(sound.currentTime, 10);
assert.equal(sound.paused, true);
sound.seeking = true;
assert.equal(movie.movieReady(soundId), false);
sound.seeking = false;
assert.equal(movie.movieReady(soundId), true);
sound.duration = 20;
sound.onloadedmetadata();
assert.equal(movie.movieError(soundId), 'audio: cache/header duration mismatch; re-export startup');
sound.error = { message: 'bad WAV' };
sound.onerror();
assert.equal(movie.movieError(soundId), 'audio: bad WAV');
sound.ended = true;
sound.currentTime = 10.9998;
movie.movieVideoEnd(soundId, 11);
const playsAtEnd = sound.playCount;
movie.playMovie(soundId);
await settle();
assert.equal(sound.playCount, playsAtEnd, 'finished audio must not restart while the final video frame arrives');
assert.equal(movie.movieTime(soundId), 11, 'audio sample rounding must not prevent video completion');
movie.closeMovie(soundId);
await settle();
assert.equal(sound.removed, true);
assert.equal(sound.loaded, true);
assert.equal(aborted, 2);
console.log('movie stream: backpressure, underruns, seeking, audio failures and cancellation passed');

const contexts = [];
globalThis.AudioContext = class {
    constructor() { this.currentTime = 0; this.destination = {}; this.buffers = []; this.sources = []; contexts.push(this); }
    createBuffer(channels, samples, rate) {
        const data = Array.from({ length: channels }, () => new Float32Array(samples));
        const buffer = { data, rate, getChannelData: channel => data[channel] };
        this.buffers.push(buffer);
        return buffer;
    }
    createBufferSource() {
        const source = { connect() {}, disconnect() {}, stop() { this.stopped = true; }, start(when, offset) { this.when = when; this.offset = offset; } };
        this.sources.push(source);
        return source;
    }
    resume() { return Promise.resolve(); }
    close() { this.closed = true; return Promise.resolve(); }
};
globalThis.cyrusGameAssets = { response: (url, options) => fetch(url, options) };
const packetId = movie.openMovie('/movies/0.smk', true, 2, 0.1);
const context = contexts.at(-1);
assert.equal(movie.movieReady(packetId), false);
movie.movieAudio(packetId, new Uint8Array([0, 255, 128, 128]), 2, 2, 8, 0, false);
assert.deepEqual([...context.buffers[0].data[0]], [-1, 0]);
assert.deepEqual([...context.buffers[0].data[1]], [127 / 128, 0]);
assert.equal(movie.movieReady(packetId), true);
movie.movieVideoEnd(packetId, 2);
movie.playMovie(packetId);
await settle();
context.currentTime = 0.5;
assert.equal(movie.movieTime(packetId), 0.5);
context.currentTime = 5;
assert.equal(movie.movieTime(packetId), 1, 'PCM underrun freezes the sample clock');
movie.movieAudio(packetId, new Uint8Array([0, 128, 255, 127]), 1, 1, 16, 1, false);
assert.deepEqual([...context.buffers[1].data[0]], [-1, 32767 / 32768]);
movie.movieTime(packetId);
await settle();
context.currentTime = 5.25;
assert.equal(movie.movieTime(packetId), 1.25, 'new PCM resumes without skipping buffered samples');
movie.seekMovie(packetId, 10);
assert.equal(movie.movieTime(packetId), 10, 'forward seek retains its target sample time');
movie.movieAudio(packetId, new Uint8Array([128, 128]), 2, 1, 8, 18, false);
assert.equal(movie.movieReady(packetId), false, 'discard PCM before the forward skip target');
movie.closeMovie(packetId);
assert.equal(context.closed, true, 'closing playback releases its audio context');
assert.ok(context.sources.every(source => source.stopped));
console.log('packet PCM: sample conversion, bounded clock, underrun resume, forward skip and cancellation passed');

const sampleRate = 44100;
const roundingId = movie.openMovie('/movies/0.smk', true, 10, 0.1);
const roundingContext = contexts.at(-1);
movie.movieAudio(roundingId, new Uint8Array(4096), sampleRate, 1, 8, 49152, false);
movie.seekMovie(roundingId, 49152 / sampleRate);
movie.movieVideoEnd(roundingId, 2);
movie.playMovie(roundingId);
await settle();
roundingContext.currentTime = 1;
movie.movieTime(roundingId);
movie.movieAudio(roundingId, new Uint8Array(4096), sampleRate, 1, 8, 53248, false);
assert.equal(movie.movieReady(roundingId), true, 'adjacent sample blocks resume after an underrun despite fractional seconds');
movie.closeMovie(roundingId);
console.log('packet PCM: exact sample boundaries resume across fractional-second underruns');
