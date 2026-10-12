const movies = new Map();
let nextId = 1;
const BUFFER_BYTES = 2 * 1024 * 1024;

export function openMovie(videoUrl, hasAudio, duration, frameSeconds) {
    const id = nextId++;
    const audio = hasAudio ? new Audio(videoUrl.replace(/\.smk$/, '.wav')) : null;
    const movie = {
        audio, chunks: [], bytes: 0, complete: false, error: null,
        controller: new AbortController(), wake: null, disposed: false,
        playing: false, playPending: false, position: 0, origin: 0, videoEnd: 0,
    };
    movies.set(id, movie);
    if (audio) {
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
            const response = await fetch(videoUrl, { signal: movie.controller.signal });
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
        movie.audio.removeAttribute('src');
        movie.audio.load();
    }
    movies.delete(id);
}
