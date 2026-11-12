// Exercise the public packet-movie API against a real WebAudio clock and PCM graph.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync} from 'node:fs';
import {connectCdp} from './capture-cdp.mjs';

const [endpoint, page, reference, output] = process.argv.slice(2);
assert(endpoint && page && reference && output, 'usage: node test-browser-movie-clock.mjs CDP_URL PAGE_URL REFERENCE_WAV RESULT_JSON');
const source = readFileSync(new URL('../../crates/cyrus-startup/src/movie_stream.js', import.meta.url), 'utf8');
const graph = readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8');
const wav = readFileSync(reference);
const cdp = await connectCdp(endpoint);
try {
    let navigated = false;
    for (let attempt = 0; attempt < 40; attempt++) {
        const navigation = await cdp.call('Page.navigate', {url: page});
        if (!navigation.errorText) { navigated = true; break; }
        await new Promise(resolve => setTimeout(resolve, 250));
    }
    assert(navigated, 'movie clock acceptance server was not ready');
    await new Promise(resolve => setTimeout(resolve, 500));
    const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {
        awaitPromise: true, returnByValue: true, userGesture: true,
        expression: `(async () => {
            ${graph}
            const contexts = [], starts = [];
            const NativeContext = AudioContext;
            globalThis.AudioContext = class extends NativeContext {
                constructor() { super(); contexts.push(this); }
                createBufferSource() {
                    const node = super.createBufferSource(), connect = node.connect, start = node.start;
                    node.connect = function (...args) {
                        const result = connect.apply(this, args);
                        // Model work between the position read and source submission.
                        const end = performance.now() + (starts.length ? 25 : 0);
                        while (performance.now() < end) {}
                        return result;
                    };
                    node.start = function (when, offset) {
                        starts.push({when, offset, submitted: this.context.currentTime});
                        return start.call(this, when, offset);
                    };
                    return node;
                }
            };
            globalThis.cyrusGameAssets = {response: async () => new Response(new Uint8Array())};
            const api = await import(URL.createObjectURL(new Blob([${JSON.stringify(source)}], {type:'text/javascript'})));
            const decoder = new NativeContext();
            const bytes = Uint8Array.from(atob(${JSON.stringify(wav.toString('base64'))}), c => c.charCodeAt(0));
            const decoded = await decoder.decodeAudioData(bytes.buffer);
            await decoder.close();
            const rate = decoded.sampleRate, total = decoded.length;
            const id = api.openMovie('/clock.smk', true, total / rate, 1 / rate);
            function push(begin, end) {
                const pcm = new Uint8Array((end - begin) * 4), view = new DataView(pcm.buffer);
                for (let i = begin; i < end; i++) for (let ch = 0; ch < 2; ch++) {
                    const value = decoded.getChannelData(Math.min(ch, decoded.numberOfChannels - 1))[i];
                    view.setInt16((i - begin) * 4 + ch * 2, Math.max(-32768, Math.min(32767, Math.round(value * 32768))), true);
                }
                api.movieAudio(id, pcm, rate, 2, 16, begin, end === total);
            }
            const split = Math.floor(total / 2);
            push(0, split);
            api.movieVideoEnd(id, total / rate);
            api.playMovie(id);
            const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
            await wait(60);
            push(split, total);
            await wait((total / rate + 0.2) * 1000);
            const origin = starts[0].when - starts[0].offset;
            const continuationError = starts[1].when - origin - split / rate;
            const time = api.movieTime(id), probe = __sessionSamples;
            // The shared observer records full submitted buffers. Packet movies
            // may start within one, so compare only the PCM actually requested.
            for (let i = 0; i < probe.buffers.length; i++) {
                const offset = Math.round(starts[i].offset * rate);
                probe.buffers[i].left = probe.buffers[i].left.subarray(offset);
                probe.buffers[i].right = probe.buffers[i].right.subarray(offset);
            }
            const active = probe.blocks.filter(b => b.frame >= Math.ceil((origin + 0.1) * rate / 128) * 128 && b.frame < Math.floor((origin + total / rate - 0.05) * rate / 128) * 128);
            if (!active.length) throw Error(JSON.stringify({starts, state: contexts[0].state, clock: probe.clock(), status: probe.status(), blocks: probe.blocks.length, rate, total}));
            const rendered = probe.match(active[0].frame, active.at(-1).frame + 128);
            api.closeMovie(id);
            return {rate, total, starts, continuationError, time, rendered};
        })()`,
    });
    assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
    writeFileSync(output, JSON.stringify({moduleSha256: createHash('sha256').update(source).digest('hex'), referenceSha256: createHash('sha256').update(wav).digest('hex'), ...result.value}, null, 2) + '\n');
    assert(Math.abs(result.value.continuationError) < 1 / result.value.rate, 'continuation source lost its sample-clock origin');
    assert(Math.abs(result.value.time - result.value.total / result.value.rate) < 1 / result.value.rate, 'movie did not reach its PCM endpoint');
    console.log(JSON.stringify(result.value));
} finally { await cdp.close(); }
