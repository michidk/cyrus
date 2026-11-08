import {afterEach, expect, test} from 'bun:test';
import {createMusic, updateMusic, musicFrames, musicEnded, stopMusic} from '../src/music/browser.js';

const sources = [];
let context;
globalThis.document = {addEventListener() {}};
globalThis.AudioContext = class {
    constructor() {
        this.currentTime = 0;
        this.sampleRate = 44100;
        this.state = 'running';
        context = this;
    }
    createBuffer(channels, length, rate) {
        return {length, sampleRate: rate, copyToChannel() {}};
    }
    createGain() { return {gain: {value: 1}, connect() {}, disconnect() {}}; }
    createBufferSource() {
        const source = {
            loop: true, connect() {}, disconnect() {},
            start(when, offset) { this.offset = offset; },
            stop() { this.onended?.(); },
        };
        sources.push(source);
        return source;
    }
    resume() { return Promise.resolve(); }
};
const players = [];
afterEach(() => {
    for (const id of players.splice(0)) stopMusic(id);
    sources.length = 0;
});
function track(paused = false) {
    const id = createMusic(new Float32Array(44100), new Float32Array(44100), 44100, 1, paused);
    players.push(id);
    return id;
}

test('a completed CD buffer stays stopped until its owner creates another playback', () => {
    const id = track();
    expect(sources[0].loop).toBe(false);
    context.currentTime += 2;
    sources[0].onended();
    expect(musicFrames(id)).toBe(44100);
    expect(musicEnded(id)).toBe(true);
    updateMusic(id, 0.5, false);
    expect(sources.length).toBe(1);
});

test('pause and resume retain progress without treating a stopped source as completed', () => {
    const id = track();
    context.currentTime += 0.25;
    updateMusic(id, 0.5, true);
    expect(musicFrames(id)).toBe(11025);
    expect(musicEnded(id)).toBe(false);
    context.currentTime += 10;
    updateMusic(id, 0.5, false);
    expect(sources[1].offset).toBe(0.25);
    expect(musicFrames(id)).toBe(11025);
    context.currentTime += 0.25;
    expect(musicFrames(id)).toBe(22050);
});

test('queued paused playback starts once and reports completion without wrapping', () => {
    const id = track(true);
    expect(sources.length).toBe(0);
    expect(musicFrames(id)).toBe(0);
    updateMusic(id, 1, false);
    context.currentTime += 3;
    expect(musicFrames(id)).toBe(44100);
    sources[0].onended();
    updateMusic(id, 1, true);
    updateMusic(id, 1, false);
    expect(sources.length).toBe(1);
});
