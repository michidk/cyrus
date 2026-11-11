// Injected before navigation by test-browser-session-cd.mjs. Tap the live
// destination gain in parallel; the tap's silent output never feeds back.
(() => {
  const probe = globalThis.__sessionCd = { sources: [], blocks: [], error: null };
  const start = AudioBufferSourceNode.prototype.start;
  const stop = AudioBufferSourceNode.prototype.stop;
  const disconnect = AudioBufferSourceNode.prototype.disconnect;
  const connect = AudioNode.prototype.connect;
  const destinations = new WeakSet();
  const owners = new WeakMap();
  const gains = new WeakMap();
  const taps = new WeakMap();
  const worklet = URL.createObjectURL(new Blob([`
    const zero = new Float32Array(128);
    registerProcessor('session-cd-tap', class extends AudioWorkletProcessor {
      process([input]) {
        this.port.postMessage({frame: currentFrame, left: input[0] ?? zero, right: input[1] ?? zero});
        return true;
      }
    });
  `], { type: 'text/javascript' }));
  AudioNode.prototype.connect = function (target, ...args) {
    const result = connect.call(this, target, ...args);
    if (this instanceof GainNode && target === this.context.destination) destinations.add(this);
    if (this instanceof AudioBufferSourceNode && target instanceof GainNode) gains.set(this, target);
    return result;
  };
  function clock(context) {
    return Math.round(context.currentTime * context.sampleRate);
  }
  AudioBufferSourceNode.prototype.start = function (...args) {
    const result = start.apply(this, args);
    const gain = gains.get(this);
    if (!this.loop || this.buffer?.numberOfChannels !== 2 || !destinations.has(gain)) return result;
    const context = this.context;
    const source = {
      frame: clock(context), rate: context.sampleRate, offset: args[1] ?? 0,
      buffer: this.buffer, gain, stopped: null, disconnected: null,
    };
    owners.set(this, source);
    probe.sources.push(source);
    let tap = taps.get(context);
    if (!tap) {
      tap = {};
      taps.set(context, tap);
      tap.ready = context.audioWorklet.addModule(worklet).then(() => {
        tap.node = new AudioWorkletNode(context, 'session-cd-tap', {
          channelCount: 2, channelCountMode: 'explicit',
        });
        tap.node.port.onmessage = ({data}) => {
          probe.blocks.push(data);
          // Retain a bounded window, including a potentially slow load.
          if (probe.blocks.length > 24000) probe.blocks.splice(0, 1000);
        };
        connect.call(tap.node, context.destination);
        probe.ready = true;
      });
    }
    if (tap.node) connect.call(gain, tap.node);
    else tap.ready.then(() => {
      if (source.disconnected === null) connect.call(gain, tap.node);
    }).catch(error => { probe.error = String(error); });
    return result;
  };
  AudioBufferSourceNode.prototype.stop = function (...args) {
    const result = stop.apply(this, args);
    const source = owners.get(this);
    if (source?.buffer) source.stopped = clock(this.context);
    return result;
  };
  AudioBufferSourceNode.prototype.disconnect = function (...args) {
    const result = disconnect.apply(this, args);
    const source = owners.get(this);
    if (source?.buffer) source.disconnected = clock(this.context);
    return result;
  };
  probe.status = () => ({
    error: probe.error,
    ready: probe.ready,
    sameClock: probe.sources.every(s => s.gain.context === probe.sources[0].gain.context),
    sources: probe.sources.map(s => ({
      frame: s.frame, now: clock(s.gain.context), rate: s.rate, bufferRate: s.buffer.sampleRate,
      length: s.buffer.length, offset: s.offset, stopped: s.stopped, disconnected: s.disconnected,
    })),
  });
  function rendered(start, end) {
    const blocks = probe.blocks.filter(b => b.frame >= start && b.frame < end);
    if (!blocks.length || blocks[0].frame !== start) throw Error('missing first rendered quantum');
    let next = start;
    for (const block of blocks) {
      if (block.frame !== next || block.left.length !== 128 || block.right?.length !== 128) {
        throw Error('missing, duplicate or malformed stereo render quantum');
      }
      next += 128;
    }
    if (next !== end) throw Error('missing last rendered quantum');
    return blocks;
  }
  probe.transition = (start, end, silentStart, silentEnd) => {
    const blocks = rendered(start, end);
    let silentFrames = 0;
    for (const block of blocks) {
      if (block.frame < silentStart || block.frame >= silentEnd) continue;
      if (block.left.some(x => x !== 0) || block.right.some(x => x !== 0)) {
        throw Error('retired CD still audible before replacement');
      }
      silentFrames += 128;
    }
    return { frames: end - start, silentFrames };
  };
  // Compare both rendered channels against the original retained source, not
  // the replacement buffer. Allow WebAudio a render quantum of start skew.
  probe.match = (index, start, end, expected, originalOffset) => {
    const source = probe.sources[index];
    const reference = probe.sources[0].buffer;
    const blocks = rendered(start, end);
    const channels = [reference.getChannelData(0), reference.getChannelData(1)];
    const gain = source.gain.gain.value;
    const mod = frame => ((frame % reference.length) + reference.length) % reference.length;
    function compare(skew, selected) {
      let error = 0, energy = 0;
      for (const {frame, left, right} of selected) {
        let cursor = mod(originalOffset + frame - expected + skew);
        for (let i = 0; i < 128; i++) {
          const l = channels[0][cursor] * gain;
          const r = channels[1][cursor] * gain;
          error += (left[i] - l) ** 2 + (right[i] - r) ** 2;
          energy += left[i] ** 2 + right[i] ** 2;
          if (++cursor === reference.length) cursor = 0;
        }
      }
      return {error, energy};
    }
    const calibration = blocks.filter((_, i) => i % Math.max(1, Math.floor(blocks.length / 32)) === 0);
    let skew = 0, best = Infinity;
    for (let candidate = -128; candidate <= 128; candidate++) {
      const {error} = compare(candidate, calibration);
      if (error < best) { best = error; skew = candidate; }
    }
    const {error, energy} = compare(skew, blocks);
    if (!(energy > 0.01 && error / energy < 1e-6)) throw Error(`rendered CD mismatch: ${error}/${energy}, skew ${skew}`);
    return {frames: end - start, skew, error, energy};
  };
})();
