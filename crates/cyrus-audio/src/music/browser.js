const players = new Map();
let nextId = 1;
let context;

function audioContext() {
    if (!context) {
        context = new AudioContext({sampleRate: 44100, latencyHint: 'playback'});
        const resume = () => {
            if (context.state === 'suspended') context.resume().catch(console.error);
        };
        document.addEventListener('pointerdown', resume);
        document.addEventListener('keydown', resume);
    }
    return context;
}

function position(player) {
    const elapsed = player.source ? Math.max(0, player.context.currentTime - player.started) : 0;
    return (player.offset + Math.floor(Math.round(elapsed * player.context.sampleRate) * player.buffer.sampleRate / player.context.sampleRate)) % player.buffer.length;
}

function play(player) {
    const source = player.context.createBufferSource();
    source.buffer = player.buffer;
    source.loop = true;
    source.connect(player.gain);
    player.started = player.context.currentTime;
    source.start(0, player.offset / player.buffer.sampleRate);
    player.source = source;
}

export function createMusic(left, right, rate, volume, paused) {
    const ctx = audioContext();
    const buffer = ctx.createBuffer(2, left.length, rate);
    buffer.copyToChannel(left, 0);
    buffer.copyToChannel(right, 1);
    const gain = ctx.createGain();
    gain.gain.value = volume;
    gain.connect(ctx.destination);
    const player = {context: ctx, buffer, gain, offset: 0, source: null, started: 0};
    if (!paused) {
        play(player);
        ctx.resume().catch(console.error);
    }
    const id = nextId++;
    players.set(id, player);
    return id;
}

export function updateMusic(id, volume, paused) {
    const player = players.get(id);
    if (!player) throw new Error('CD playback owner is absent');
    player.gain.gain.value = volume;
    if (paused && player.source) {
        player.offset = position(player);
        player.source.stop();
        player.source.disconnect();
        player.source = null;
    } else if (!paused && !player.source) {
        play(player);
        player.context.resume().catch(console.error);
    }
}

export function musicFrames(id) {
    const player = players.get(id);
    if (!player) throw new Error('CD playback owner is absent');
    return position(player);
}

export function stopMusic(id) {
    const player = players.get(id);
    if (!player) return;
    player.prepared?.disconnect();
    player.source?.stop();
    player.source?.disconnect();
    player.gain.disconnect();
    players.delete(id);
}

// Build the replacement graph without starting its clock or changing live output.
export function prepareMusic(left, right, rate, volume) {
    const id = createMusic(left, right, rate, volume, true);
    const player = players.get(id);
    const source = player.context.createBufferSource();
    source.buffer = player.buffer;
    source.loop = true;
    source.connect(player.gain);
    player.prepared = source;
    return id;
}

// Called only after the complete session's publication preflight succeeds.
export function publishMusic(id, retiring, paused) {
    const player = players.get(id);
    if (!player?.prepared) throw new Error('prepared CD output is absent');
    if (player.context.state === 'closed') throw new Error('CD output context is closed');
    const when = player.context.currentTime;
    if (!paused) {
        player.prepared.start(when, 0);
        player.source = player.prepared;
        player.started = when;
        player.context.resume().catch(console.error);
    } else {
        player.prepared.disconnect();
    }
    player.prepared = null;
    for (const old of retiring) {
        const previous = players.get(old);
        if (!previous) continue;
        previous.source?.stop(when);
        previous.source?.disconnect();
        previous.gain.disconnect();
        players.delete(old);
    }
}
