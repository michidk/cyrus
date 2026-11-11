// Injected before navigation by test-browser-session-cd.mjs. Tap the live
// destination gain in parallel; the tap's silent output never feeds back.
(() => {
  const probe = globalThis.__sessionCd = { sources: [], blocks: [], error: null };
  const createBuffer = AudioContext.prototype.createBuffer;
  const copy = AudioBuffer.prototype.copyToChannel;
  const buffers = new WeakMap();
  AudioContext.prototype.createBuffer = function (...args) {
    const began = Math.round(this.currentTime * this.sampleRate);
    const buffer = createBuffer.apply(this, args);
    buffers.set(buffer, {context: this, began, copied: null});
    return buffer;
  };
  AudioBuffer.prototype.copyToChannel = function (...args) {
    const result = copy.apply(this, args);
    const buffer = buffers.get(this);
    if (buffer) buffer.copied = Math.round(buffer.context.currentTime * buffer.context.sampleRate);
    return result;
  };
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
      prepared: buffers.get(this.buffer), frame: args[0] > 0 ? Math.round(args[0] * context.sampleRate) : clock(context), rate: context.sampleRate, offset: args[1] ?? 0,
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
    if (source?.buffer) source.stopped = args[0] > 0 ? Math.round(args[0] * this.context.sampleRate) : clock(this.context);
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
      preparationBegan: s.prepared?.began, preparationCopied: s.prepared?.copied,
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
  // Check the complete switch against the two independently calibrated phases.
  probe.transition = (start, end, cut, oldStart, oldSkew, savedCursor) => {
    const blocks = rendered(start, end);
    const reference = probe.sources[0].buffer;
    const channels = [reference.getChannelData(0), reference.getChannelData(1)];
    let error = 0, energy = 0;
    for (const {frame, left, right} of blocks) {
      for (let i = 0; i < 128; i++) {
        const current = frame + i;
        const phase = current < cut ? current - oldStart + oldSkew : savedCursor + current - cut;
        const cursor = ((phase % reference.length) + reference.length) % reference.length;
        const gain = probe.sources[current < cut ? 0 : 1].gain.gain.value;
        const l = channels[0][cursor] * gain, r = channels[1][cursor] * gain;
        error += (left[i] - l) ** 2 + (right[i] - r) ** 2;
        energy += left[i] ** 2 + right[i] ** 2;
      }
    }
    if (!(energy > 0.01 && error / energy < 1e-6)) throw Error(`CD switch mismatch: ${error}/${energy}`);
    return {frames: end - start, cut, error, energy};
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
