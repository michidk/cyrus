// The codec supplies bounded PCM blocks; Web Audio owns playback and its sample clock.
class PacketAudio {
    constructor(duration, frameSeconds) {
        this.context = new AudioContext();
        this.duration = duration;
        this.frameSeconds = frameSeconds;
        this.blocks = [];
        this.sources = [];
        this.position = 0;
        this.bufferEnd = 0;
        this.paused = true;
        this.seeking = false;
        this.complete = false;
    }
    get currentTime() {
        if (this.paused) return this.position;
        const time = this.position + this.context.currentTime - this.origin;
        if (time >= this.bufferEnd) { this.pause(this.bufferEnd); return this.position; }
        return time;
    }
    set currentTime(seconds) {
        this.pause();
        this.position = seconds;
        this.blocks = this.blocks.filter(block => block.end > seconds);
    }
    get ended() { return this.complete && this.currentTime >= this.bufferEnd; }
    get readyState() { return this.blocks.some(block => block.start <= this.position && block.end > this.position) || this.ended ? 2 : 0; }
    push(pcm, rate, channels, bits, startSample, last) {
        if (!(rate > 0) || ![1, 2].includes(channels) || ![8, 16].includes(bits)) throw new Error('Unsupported movie PCM layout');
        const alignment = channels * bits / 8;
        if (pcm.length % alignment) throw new Error('Incomplete movie PCM frame');
        const samples = pcm.length / alignment;
        const start = startSample / rate;
        const end = (startSample + samples) / rate;
        this.bufferEnd = Math.max(this.bufferEnd, end);
        if (last) {
            this.complete = true;
            if (Math.abs(end - this.duration) > this.frameSeconds) throw new Error('audio: cache/header duration mismatch; re-export startup');
        }
        if (!samples || end <= this.position) return;
        const buffer = this.context.createBuffer(channels, samples, rate);
        const view = new DataView(pcm.buffer, pcm.byteOffset, pcm.byteLength);
        for (let channel = 0; channel < channels; channel++) {
            const output = buffer.getChannelData(channel);
            for (let sample = 0; sample < samples; sample++) {
                const offset = (sample * channels + channel) * bits / 8;
                output[sample] = bits === 8 ? (pcm[offset] - 128) / 128 : view.getInt16(offset, true) / 32768;
            }
        }
        const block = { start, end, buffer };
        this.blocks.push(block);
        if (!this.paused) this.schedule(block);
    }
    schedule(block) {
        const position = this.currentTime;
        if (block.end <= position) return;
        const source = this.context.createBufferSource();
        source.buffer = block.buffer;
        source.connect(this.context.destination);
        const offset = Math.max(0, position - block.start);
        source.start(this.context.currentTime + Math.max(0, block.start - position), offset);
        this.sources.push(source);
        source.onended = () => {
            this.sources = this.sources.filter(active => active !== source);
            this.blocks = this.blocks.filter(pending => pending !== block);
            source.disconnect();
        };
    }
    async play() {
        await this.context.resume();
        if (!this.paused) return;
        this.paused = false;
        this.origin = this.context.currentTime;
        for (const block of this.blocks) this.schedule(block);
    }
    pause(position = this.currentTime) {
        if (this.paused) return;
        this.position = position;
        this.paused = true;
        for (const source of this.sources) { source.onended = null; source.stop(); source.disconnect(); }
        this.sources = [];
        this.blocks = this.blocks.filter(block => block.end > this.position);
    }
    close() { this.pause(); this.blocks = []; void this.context.close(); }
}

const movies = new Map();
let nextId = 1;
const BUFFER_BYTES = 2 * 1024 * 1024;

export function openMovie(videoUrl, hasAudio, duration, frameSeconds) {
    const id = nextId++;
    const prepared = globalThis.cyrusGameAssets;
    const audio = hasAudio ? (prepared ? new PacketAudio(duration, frameSeconds) : new Audio(videoUrl.replace(/\.smk$/, '.wav'))) : null;
    const movie = {
        audio, chunks: [], bytes: 0, complete: false, error: null,
        controller: new AbortController(), wake: null, disposed: false,
        playing: false, playPending: false, position: 0, origin: 0, videoEnd: 0,
    };
    movies.set(id, movie);
    if (audio && !prepared) {
        audio.preload = 'auto';
        audio.onloadedmetadata = () => {
            if (!Number.isFinite(audio.duration) || Math.abs(audio.duration - duration) > frameSeconds) {
                movie.error = 'audio: cache/header duration mismatch; re-export startup';
            }
        };
        audio.onerror = () => { movie.error = `audio: ${audio.error?.message || 'load failed'}`; };
    }
    (async () => {
        try {
            const response = await (prepared ? prepared.response(videoUrl, { signal: movie.controller.signal }) : fetch(videoUrl, { signal: movie.controller.signal }));
            if (!response.ok) throw new Error(`video: HTTP ${response.status}`);
            if (!response.body) throw new Error('video: streaming response unavailable');
            const reader = response.body.getReader();
            for (;;) {
                if (movie.disposed) return;
                if (movie.bytes >= BUFFER_BYTES) {
                    await new Promise(resolve => { movie.wake = resolve; });
                    continue;
                }
                const { done, value } = await reader.read();
                if (done) { movie.complete = true; return; }
                // Bound the Rust-facing chunks even when the browser supplies a large read.
                for (let offset = 0; offset < value.length; offset += 256 * 1024) {
                    while (movie.bytes >= BUFFER_BYTES && !movie.disposed) {
                        await new Promise(resolve => { movie.wake = resolve; });
                    }
                    if (movie.disposed) return;
                    const chunk = value.subarray(offset, offset + 256 * 1024);
                    movie.chunks.push(chunk);
                    movie.bytes += chunk.length;
                }
            }
        } catch (error) {
            if (!movie.disposed) movie.error = String(error);
        }
    })();
    return id;
}
export function movieAudio(id, pcm, rate, channels, bits, startSample, last) {
    const audio = movies.get(id)?.audio;
    if (audio instanceof PacketAudio) audio.push(pcm, rate, channels, bits, startSample, last);
}
export function movieChunk(id) {
    const movie = movies.get(id);
    const chunk = movie.chunks.shift();
    if (!chunk) return null;
    movie.bytes -= chunk.length;
    const wake = movie.wake;
    movie.wake = null;
    wake?.();
    return chunk;
}
export function movieError(id) { return movies.get(id).error; }
export function movieComplete(id) { return movies.get(id).complete; }
export function movieReady(id) {
    const audio = movies.get(id).audio;
    return !audio || (audio.readyState >= 2 && !audio.seeking);
}
function resume(movie) {
    if (!movie.playing || !movie.audio || movie.audio.ended || movie.playPending || !movie.audio.paused ||
        movie.audio.readyState < 2 || movie.audio.seeking || movie.audio.currentTime >= movie.videoEnd) return;
    movie.playPending = true;
    movie.audio.play().then(() => {
        if (!movie.playing || movie.disposed) movie.audio.pause();
    }).catch(error => {
        if (!movie.disposed && error.name !== 'AbortError') movie.error = `audio playback: ${error}`;
    }).finally(() => { movie.playPending = false; });
}
export function movieVideoEnd(id, end) {
    const movie = movies.get(id);
    if (!movie.audio && movie.playing && movieTime(id) >= movie.videoEnd) {
        movie.position = movie.videoEnd;
        movie.origin = performance.now();
    }
    movie.videoEnd = end;
    resume(movie);
}
export function movieTime(id) {
    const movie = movies.get(id);
    const time = movie.audio ? (movie.audio.ended ? movie.videoEnd : movie.audio.currentTime) : movie.position +
        (movie.playing ? (performance.now() - movie.origin) / 1000 : 0);
    if (movie.audio && time >= movie.videoEnd) movie.audio.pause();
    resume(movie);
    return Math.min(time, movie.videoEnd);
}
export function playMovie(id) {
    const movie = movies.get(id);
    if (!movie.playing) {
        movie.origin = performance.now();
        movie.playing = true;
    }
    resume(movie);
}
export function seekMovie(id, seconds) {
    const movie = movies.get(id);
    movie.playing = false;
    movie.position = seconds;
    movie.videoEnd = seconds;
    if (movie.audio) {
        movie.audio.pause();
        movie.audio.currentTime = seconds;
    }
}
export function closeMovie(id) {
    const movie = movies.get(id);
    if (!movie) return;
    movie.disposed = true;
    movie.playing = false;
    movie.controller.abort();
    movie.wake?.();
    if (movie.audio) {
        movie.audio.pause();
        if (movie.audio instanceof PacketAudio) movie.audio.close();
        else { movie.audio.removeAttribute('src'); movie.audio.load(); }
    }
    movies.delete(id);
}
