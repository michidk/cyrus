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
    return Math.min(player.offset + Math.floor(Math.round(elapsed * player.context.sampleRate) * player.buffer.sampleRate / player.context.sampleRate), player.buffer.length);
}

function play(player) {
    const source = player.context.createBufferSource();
    source.buffer = player.buffer;
    source.loop = false;
    source.connect(player.gain);
    player.started = player.context.currentTime;
    source.start(0, player.offset / player.buffer.sampleRate);
    player.source = source;
    source.onended = () => {
        if (player.source === source) {
            player.ended = true;
            player.offset = player.buffer.length;
            player.source = null;
            source.disconnect();
        }
    };
}

export function createMusic(left, right, rate, volume, paused) {
    const ctx = audioContext();
    const buffer = ctx.createBuffer(2, left.length, rate);
    buffer.copyToChannel(left, 0);
    buffer.copyToChannel(right, 1);
    const gain = ctx.createGain();
    gain.gain.value = volume;
    gain.connect(ctx.destination);
    const player = {context: ctx, buffer, gain, offset: 0, source: null, started: 0, ended: false};
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
        const source = player.source;
        player.source = null;
        source.stop();
        source.disconnect();
    } else if (!paused && !player.source && !player.ended) {
        play(player);
    }
}

export function musicFrames(id) {
    const player = players.get(id);
    if (!player) throw new Error('CD playback owner is absent');
    return Math.min(position(player), player.buffer.length);
}

export function stopMusic(id) {
    const player = players.get(id);
    if (!player) return;
    player.source?.stop();
    player.source?.disconnect();
    player.gain.disconnect();
    players.delete(id);
}

export function musicEnded(id) {
    const player = players.get(id);
    if (!player) throw new Error('CD playback owner is absent');
    return player.ended;
}
