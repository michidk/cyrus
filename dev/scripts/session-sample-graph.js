// Tap the production CPAL stream in parallel. Mixed buffers may drain after a
// sample owner retires; compare rendered PCM with the buffers actually submitted.
(() => {
  const probe = globalThis.__sessionSamples = { buffers: [], blocks: [], error: null };
  const original = AudioBufferSourceNode.prototype.start;
  const connect = AudioNode.prototype.connect;
  let context, tap, ready;
  const worklet = URL.createObjectURL(new Blob([`
    registerProcessor('session-sample-tap', class extends AudioWorkletProcessor {
      process([input]) {
        this.port.postMessage({frame: currentFrame, left: input[0] ?? new Float32Array(128), right: input[1] ?? new Float32Array(128)});
        return true;
      }
    });
  `], {type: 'text/javascript'}));
  AudioBufferSourceNode.prototype.start = function (...args) {
    const result = original.apply(this, args);
    try {
      if (probe.error) return result;
      if (this.loop || this.buffer?.numberOfChannels !== 2) return result;
      if (context && context !== this.context) { probe.error = 'multiple sample stream clocks'; return result; }
      context = this.context;
      if (!context.audioWorklet) throw Error('AudioWorklet unavailable; capture requires a secure origin');
      if (!ready) ready = context.audioWorklet.addModule(worklet).then(() => {
        tap = new AudioWorkletNode(context, 'session-sample-tap', {channelCount: 2, channelCountMode: 'explicit'});
        tap.port.onmessage = ({data}) => {
          probe.blocks.push(data);
          if (probe.blocks.length > 24000) probe.blocks.splice(0, 1000);
        };
        connect.call(tap, context.destination);
        // Keep the observer processing silent quanta between scheduled buffers.
        const silence = context.createConstantSource();
        silence.offset.value = 0;
        connect.call(silence, tap);
        silence.start();
      });
      if (tap) connect.call(this, tap);
      else ready.then(() => connect.call(this, tap)).catch(error => {probe.error = String(error)});
      probe.buffers.push({frame: (args[0] || context.currentTime) * context.sampleRate,
        left: new Float32Array(this.buffer.getChannelData(0)), right: new Float32Array(this.buffer.getChannelData(1))});
    } catch (error) {
      probe.error = String(error);
    }
    return result;
  };
  probe.status = () => ({error: probe.error, ready: !!tap && !probe.error, rate: context?.sampleRate,
    now: context && Math.round(context.currentTime * context.sampleRate),
    buffers: probe.buffers.map(b => ({frame: b.frame, length: b.left.length,
      energy: b.left.reduce((sum, x) => sum + x*x, 0) + b.right.reduce((sum, x) => sum + x*x, 0)})),
    last: probe.blocks.at(-1)?.frame});
  probe.match = (start, end, shift = 0) => {
    if (probe.error) throw Error(`sample graph rejected: ${probe.error}`);
    const blocks = probe.blocks.filter(b => b.frame >= start && b.frame < end);
    let next = start;
    for (const block of blocks) { if (block.frame !== next) throw Error(`missing or duplicate sample graph quantum: expected ${next}, got ${block.frame}, window ${start}..${end}`); next += 128; }
    if (next !== end) throw Error('incomplete sample graph window');
    function compare(skew, selected) {
      let error = 0, energy = 0; const differences = [];
      for (const {frame, left, right} of selected) {
        const active = probe.buffers.filter(b => b.frame < frame+256 && b.frame+b.left.length > frame-128);
        for (let i = 0; i < 128; i++) {
          const at = frame + i + skew + shift;
          let l = 0, r = 0;
          for (const buffer of active) {
            const offset = at - buffer.frame;
            if (offset >= 0 && offset < buffer.left.length) {
              // Fractional starts interpolate PCM, including the final segment.
              // The installed Chromium OfflineAudioContext control covers that edge.
              const index = Math.floor(offset), fraction = offset-index;
              l += buffer.left[index]*(1-fraction) + (buffer.left[index+1] ?? (2*buffer.left[index]-buffer.left[index-1]))*fraction;
              r += buffer.right[index]*(1-fraction) + (buffer.right[index+1] ?? (2*buffer.right[index]-buffer.right[index-1]))*fraction;
            }
          }
          const delta = (left[i]-l)**2 + (right[i]-r)**2;
          error += delta;
          if (delta > 1e-8 && differences.length < 12) differences.push({at, l, r, left: left[i], right: right[i], active: active.map(b => ({frame:b.frame,length:b.left.length}))});
          energy += left[i]**2 + right[i]**2;
        }
      }
      return {error, energy, differences};
    }
    let best = {error: Infinity}, skew;
    for (let candidate = -128; candidate <= 128; candidate++) {
      const value = compare(candidate, blocks.filter((_, i) => i % Math.max(1, Math.floor(blocks.length/8)) === 0));
      if (value.error < best.error) {best = value; skew = candidate;}
      if (best.error === 0) break;
    }
    best = compare(skew, blocks);
    if (!(best.energy > 0.001 && best.error / best.energy < 1e-6)) throw Error(`sample graph mismatch ${JSON.stringify({skew, ...best})}`);
    return {frames: end-start, skew, ...best};
  };
})();
