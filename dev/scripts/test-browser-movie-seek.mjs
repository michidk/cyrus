// Installed decoded cinematic packets exercise the production browser clock API.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync, readFileSync, writeFileSync} from 'node:fs';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint, page, windowUrl, output] = process.argv.slice(2);
assert(endpoint && page && windowUrl && output, 'usage: node test-browser-movie-seek.mjs CDP_URL PAGE_URL WINDOW_URL RESULT_JSON');
assert(!existsSync(output), 'output exists; preserve evidence and choose a new result path');
const source = readFileSync(new URL('../../crates/cyrus-startup/src/movie_stream.js', import.meta.url), 'utf8');
const graph = readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8');
const cdp = await connectCdp(endpoint);
try {
    let navigated = false;
    for (let i = 0; i < 40; i++) {
        if (!(await cdp.call('Page.navigate', {url: page})).errorText) {navigated = true; break;}
        await new Promise(resolve => setTimeout(resolve, 250));
    }
    assert(navigated, 'acceptance server unavailable');
    await new Promise(resolve => setTimeout(resolve, 500));
    const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {
        awaitPromise: true, returnByValue: true, userGesture: true,
        expression: `(async () => {
            const nativeStart = AudioBufferSourceNode.prototype.start;
            ${graph}
            const check = (value, message) => {if (!value) throw Error(message);};
            const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
            const root = new URL('./', ${JSON.stringify(windowUrl)});
            const windowData = await (await fetch(${JSON.stringify(windowUrl)})).json();
            const frameSeconds = windowData.metadata.frame_micros / 1000000;
            const packets = await Promise.all(windowData.frames.map(async f => ({...f,
                pcm: f.audio && new Uint8Array(await (await fetch(new URL(f.audio.file, root))).arrayBuffer()),
                rgba: new Uint8ClampedArray(await (await fetch(new URL(f.picture, root))).arrayBuffer()),
            })));
            const contexts = [], sources = [];
            const NativeContext = AudioContext;
            globalThis.AudioContext = class extends NativeContext {
                constructor() {super(); contexts.push(this);}
                createBufferSource() {
                    const node = super.createBufferSource(), start = node.start, stop = node.stop;
                    node.start = function (when, offset) {
                        sources.push({node: this, when, offset, stopped: null});
                        return start.call(this, when, offset);
                    };
                    node.stop = function () {
                        sources.find(s => s.node === this).stopped = this.context.currentTime;
                        return stop.call(this);
                    };
                    return node;
                }
            };
            globalThis.cyrusGameAssets = {response: async () => new Response(new Uint8Array())};
            const api = await import(URL.createObjectURL(new Blob([${JSON.stringify(source)}], {type:'text/javascript'})));
            const id = api.openMovie('/installed/INTRO.SMK', true,
                windowData.metadata.frames * frameSeconds, frameSeconds);
            function push(p) {if (p.audio) api.movieAudio(id, p.pcm, p.audio.rate, p.audio.channels, p.audio.bits, p.audio.start_sample, false);}
            push(packets[0]);
            const first = packets[0].audio;
            const boundary = (first.start_sample + packets[0].pcm.length / (first.channels * first.bits / 8)) / first.rate;
            api.movieVideoEnd(id, packets.length * frameSeconds);
            api.playMovie(id);
            await wait(40);
            const context = contexts[0];
            const canvas = document.createElement('canvas'); canvas.width = 640;
            canvas.height = packets[0].rgba.length / (640 * 4); document.body.replaceChildren(canvas);
            const painter = canvas.getContext('2d'), presented = [];
            async function present(label) {
                await new Promise(resolve => requestAnimationFrame(resolve));
                const time = api.movieTime(id), index = Math.floor(time / frameSeconds);
                check(index < packets.length, 'presentation escaped the bounded installed window');
                painter.putImageData(new ImageData(packets[index].rgba, canvas.width, canvas.height), 0, 0);
                presented.push({label, time, frame: packets[index].number, audio: context.currentTime});
                await new Promise(resolve => requestAnimationFrame(resolve));
            }
            await present('initial');
            // Leave no movie-clock poll between exhaustion and producer admission.
            await wait((boundary + 0.15) * 1000);
            for (const packet of packets.slice(1)) push(packet);
            const lateTime = api.movieTime(id);
            await wait(60);
            await present('late-resume');
            const lateResume = api.movieTime(id);
            const queued = sources.filter(s => s.stopped === null && s.when >= context.currentTime);
            check(queued.length > 0, 'seek control had no queued future audio');
            const target = 2;
            api.seekMovie(id, target);
            const cancelled = queued.every(s => s.stopped !== null && s.node.onended === null);
            const frozen = api.movieTime(id);
            await wait(80);
            const paused = api.movieTime(id);
            api.movieVideoEnd(id, packets.length * frameSeconds);
            api.playMovie(id);
            await wait(100);
            await present('seek-resume');
            await wait(80);
            await present('continuation');
            const end = api.movieTime(id);
            const status = __sessionSamples.status();
            check(!status.error && status.ready, status.error || 'PCM observer not ready');
            const blocks = __sessionSamples.blocks;
            for (let i = 1; i < blocks.length; i++) check(blocks[i].frame === blocks[i-1].frame + 128, 'graph quantum continuity failed');
            // Re-render the actual scheduled sources, offsets and cancellation times.
            AudioBufferSourceNode.prototype.start = nativeStart;
            const offline = new OfflineAudioContext(2, Math.ceil((context.currentTime + 0.1) * context.sampleRate), context.sampleRate);
            for (const s of sources) {
                const node = offline.createBufferSource(); node.buffer = s.node.buffer;
                node.connect(offline.destination); node.start(s.when, s.offset);
                if (s.stopped !== null) node.stop(s.stopped);
            }
            const expected = await offline.startRendering();
            const lastSeek = presented.find(p => p.label === 'seek-resume').audio;
            const selected = blocks.filter(b => b.frame >= (lastSeek + 0.02) * context.sampleRate);
            check(selected.length > 0, 'no rendered post-seek PCM');
            let error = 0, energy = 0;
            for (const b of selected) for (let i = 0; i < 128; i++) for (let ch = 0; ch < 2; ch++) {
                const actual = b[ch ? 'right' : 'left'][i], value = expected.getChannelData(ch)[b.frame + i];
                error += (actual - value) ** 2; energy += actual ** 2;
            }
            const receipt = {boundary, lateTime, lateResume, target, frozen, paused, end, cancelled,
                queued: queued.length, presented, graph: {frames: selected.length * 128, error, energy},
                sources: sources.map(s => ({when:s.when,offset:s.offset,stopped:s.stopped})),
                metadata: {frames: windowData.metadata.frames, frameSeconds, windowFrames: packets.length}};
            api.closeMovie(id);
            return receipt;
        })()`,
    });
    assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
    const receipt = {moduleSha256: createHash('sha256').update(source).digest('hex'), ...result.value};
    writeFileSync(output, JSON.stringify(receipt, null, 2) + '\n');
    assert.equal(receipt.lateTime, receipt.boundary, 'late producer admission skipped stalled movie samples');
    assert(receipt.lateResume > receipt.boundary && receipt.lateResume < receipt.boundary + 0.2, 'late PCM did not resume from its boundary');
    assert(receipt.cancelled, 'seek retained stale queued PCM');
    assert.equal(receipt.frozen, receipt.target); assert.equal(receipt.paused, receipt.target);
    assert(receipt.end > receipt.target && receipt.end < receipt.target + 0.5);
    assert(receipt.graph.energy > 0.001 && receipt.graph.error / receipt.graph.energy < 1e-6, 'post-seek rendered PCM differs from source cancellation timeline');
    for (const p of receipt.presented) assert(p.time >= (p.frame - 1) * receipt.metadata.frameSeconds && p.time < p.frame * receipt.metadata.frameSeconds);
    console.log(JSON.stringify({lateTime:receipt.lateTime, boundary:receipt.boundary, cancelled:receipt.cancelled, graph:receipt.graph, presented:receipt.presented}));
} finally {await cdp.close();}
