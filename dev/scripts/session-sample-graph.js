// Tap the production CPAL stream in parallel. Mixed buffers may drain after a
// sample owner retires; compare rendered PCM with the buffers actually submitted.
(() => {
  const probe = globalThis.__sessionSamples = { buffers: [], blocks: [], error: null, startFrame: null };
  const original = AudioBufferSourceNode.prototype.start;
  const connect = AudioNode.prototype.connect;
  let context, tap, ready;
  const pendingRetirements = [];
  probe.retirements = {};
  const accepts = source => !globalThis.__sessionSampleSourceFilter || globalThis.__sessionSampleSourceFilter(source);
  const worklet = URL.createObjectURL(new Blob([`
    registerProcessor('session-sample-tap', class extends AudioWorkletProcessor {
      static get parameterDescriptors() { return [
        {name: 'sampleFrame', defaultValue: 0, automationRate: 'a-rate'},
        {name: 'coarseFrame', defaultValue: 0, automationRate: 'a-rate'},
        {name: 'recording', defaultValue: 0, automationRate: 'a-rate'},
      ]; }
      constructor(options) {
        super(); this.armed = options?.processorOptions?.startFrame ?? 0;
        this.retirements = [];
        this.port.onmessage = ({data}) => { this.retirements.push(data.retire); };
      }
      process([input], outputs, parameters) {
        if (parameters.recording[0] !== 1) return true;
        const clock = parameters.sampleFrame;
        const length = input[0]?.length;
        const direction = clock[clock.length - 1] > clock[0] ? 1 : -1;
        const phase = direction === 1 ? Math.round(clock[0]) : 131072 - Math.round(clock[0]);
        // The coarse ramp selects a period only. The small continuous triangle
        // supplies exact sample positions without float32 absolute-ramp drift.
        const cycle = Math.round((parameters.coarseFrame[0] - this.armed - phase) / 131072);
        const frame = this.armed + cycle * 131072 + phase;
        const end = frame + direction * (Math.round(clock[clock.length - 1]) - Math.round(clock[0]));
        // Validate every fine-clock sample, not just the quantum endpoints.
        // Keep the worklet-global clock for diagnosis, never repair gaps from it.
        const error = !length || input[1]?.length !== length || clock.length !== length
          || clock.some((value, index) => direction * (Math.round(value) - Math.round(clock[0])) !== index)
          || end !== frame + length - 1 || end >= 2 ** 24;
        // A separate opt-in output carries the validated generation clock to
        // source-owned diagnostic taps. The destination output stays silent.
        if (!error && outputs[1]) console.timeStamp('session-generation:' + frame);
        if (!error && outputs[1]) for (let i = 0; i < length; i++) {
          outputs[1][0][i] = frame + i;
          outputs[1][1][i] = this.sequence ?? 0;
        }
        this.port.postMessage({frame, reportedFrame: currentFrame,
          sequence: this.sequence ?? 0, retirements: this.retirements.splice(0), error: error ? 'invalid automated sample clock' : null,
          clockError: error ? {coarse: parameters.coarseFrame[0], clock: Array.from(clock)} : null,
          left: input[0], right: input[1]});
        this.sequence = (this.sequence ?? 0) + 1;
        return true;
      }
    });
  `], {type: 'text/javascript'}));
  AudioBufferSourceNode.prototype.start = function (...args) {
    let connected = false;
    try {
      // Wire a ready observer before activation. A rendering thread can consume
      // an immediate source while the native start call is still returning.
      if (!probe.error && tap && context === this.context && accepts(this) && !this.loop
          && this.buffer?.numberOfChannels === 2) {
        connect.call(this, tap);
        connected = true;
      }
    } catch (error) { probe.error = String(error); }
    let nativeBefore;
    try { nativeBefore = this.context.currentTime; }
    catch (error) { probe.error = String(error); }
    const result = original.apply(this, args);
    try {
      const nativeAfter = this.context.currentTime;
      if (probe.error) return result;
      if (!accepts(this) || this.loop || this.buffer?.numberOfChannels !== 2) return result;
      if (context && context !== this.context) { probe.error = 'multiple sample stream clocks'; return result; }
      context = this.context;
      if (!context.audioWorklet) throw Error('AudioWorklet unavailable; capture requires a secure origin');
      if (!ready) ready = context.audioWorklet.addModule(worklet).then(() => {
        // Arm on a future render boundary after configuring both automation lanes.
        // This excludes setup, while retaining every quantum in the capture interval.
        probe.startFrame = Math.ceil((context.currentTime + 0.1) * context.sampleRate / 128) * 128;
        tap = new AudioWorkletNode(context, 'session-sample-tap', {channelCount: 2,
          channelCountMode: 'explicit', numberOfOutputs: globalThis.__sessionSampleClockOutput ? 2 : 1,
          outputChannelCount: globalThis.__sessionSampleClockOutput ? [2, 2] : [2], processorOptions: {startFrame: probe.startFrame}});
        tap.parameters.get('coarseFrame').setValueAtTime(probe.startFrame, probe.startFrame / context.sampleRate);
        tap.parameters.get('coarseFrame').linearRampToValueAtTime(2 ** 24, 2 ** 24 / context.sampleRate);
        tap.parameters.get('sampleFrame').setValueAtTime(0, probe.startFrame / context.sampleRate);
        // Continuous alternating ramps avoid discontinuous automation endpoints.
        for (let frame = probe.startFrame, up = true; frame < 2 ** 24; frame += 65536, up = !up) {
          tap.parameters.get('sampleFrame').linearRampToValueAtTime(up ? 65536 : 0, (frame + 65536) / context.sampleRate);
        }
        tap.parameters.get('recording').setValueAtTime(1, probe.startFrame / context.sampleRate);
        let nextFrame = probe.startFrame, nextSequence = 0;
        tap.port.onmessage = ({data}) => {
          if (data.error || data.frame !== nextFrame || data.sequence !== nextSequence) {
            probe.error ??= data.error || 'missing or duplicate sample graph quantum: expected '
              + nextFrame + '/' + nextSequence + ', got ' + data.frame + '/' + data.sequence;
          }
          for (const id of data.retirements ?? []) probe.retirements[id] = data.frame;
          nextFrame = data.frame + (data.left?.length ?? 0);
          nextSequence = data.sequence + 1;
          // Keep the armed interval intact through slow seek decoding. The exact
          // automated clock bounds storage to at most 2^24 stereo sample frames.
          if (probe.blocks.length < 2 ** 24 / 128) probe.blocks.push(data);
          else probe.error ??= 'sample graph capture capacity exceeded';
        };
        for (const id of pendingRetirements.splice(0)) tap.port.postMessage({retire: id});
        connect.call(tap, context.destination);
        // Keep the observer processing silent quanta between scheduled buffers.
        const silence = context.createConstantSource();
        silence.offset.value = 0;
        connect.call(silence, tap);
        silence.start();
      });
      if (tap && !connected) connect.call(this, tap);
      else if (!tap) ready.then(() => connect.call(this, tap)).catch(error => {probe.error = String(error)});
      const requested = args[0] ?? 0;
      const buffer = {frame: Math.max(requested, nativeBefore) * context.sampleRate,
        admission: {requested, nativeBefore, nativeAfter,
          kind: requested > nativeAfter ? 'scheduled' : 'immediate', endedFrame: null},
        left: new Float32Array(this.buffer.getChannelData(0)), right: new Float32Array(this.buffer.getChannelData(1))};
      // An immediate command may join the rendering graph in a later quantum.
      // Its command clock is a lower bound, not a measured consumption boundary.
      this.addEventListener?.('ended', () => {
        buffer.admission.endedFrame = (Math.ceil(this.context.currentTime * this.context.sampleRate / 128) + 1) * 128;
      }, {once: true});
      probe.buffers.push(buffer);
    } catch (error) {
      probe.error = String(error);
    }
    return result;
  };
  // Retirement is acknowledged by the rendering thread at an exact generation
  // quantum. A main-thread currentTime read is not a retirement sample boundary.
  probe.retireSource = id => {
    if (!Number.isInteger(id) || id < 0) throw Error('invalid source retirement identity');
    if (tap) tap.port.postMessage({retire: id}); else pendingRetirements.push(id);
  };
  // Deliberately unavailable unless requested before installing this observer.
  // Use the native connect method, so other diagnostic wrappers cannot reroute it.
  probe.connectClock = (node, input = 1) => {
    if (!globalThis.__sessionSampleClockOutput || !tap || probe.error) throw Error('generation clock output unavailable');
    return connect.call(tap, node, 1, input);
  };
  // Frequent clock observations must not scan every retained production buffer.
  probe.clock = () => ({rate: context?.sampleRate,
    now: context && Math.round(context.currentTime * context.sampleRate)});
  probe.status = () => ({error: probe.error, ready: !!tap && !probe.error, rate: context?.sampleRate,
    now: context && Math.round(context.currentTime * context.sampleRate),
    buffers: probe.buffers.map(b => ({frame: b.frame, admission: {...b.admission}, length: b.left.length,
      energy: b.left.reduce((sum, x) => sum + x*x, 0) + b.right.reduce((sum, x) => sum + x*x, 0)})),
    last: probe.blocks.at(-1)?.frame});
  // Freeze the sample window before asynchronous CDP readback. Diagnostics survive
  // rejection; a failed window never receives exportable PCM.
  probe.capture = (start, end) => {
    const blocks = probe.blocks.filter(b => b.frame >= start && b.frame < end);
    probe.capturedPcm = null;
    const metadata = probe.lastCapture = {start, end, rate: context?.sampleRate,
      clock: probe.clock(), armed: probe.startFrame, error: probe.error,
      sourceAdmissions: probe.buffers.map(b => ({frame: b.frame, length: b.left.length, ...b.admission})),
      quanta: blocks.map(b => ({frame: b.frame, sequence: b.sequence,
        reportedFrame: b.reportedFrame, length: b.left?.length, error: b.error, clockError: b.clockError}))};
    let next = start, sequence = blocks[0]?.sequence;
    for (const block of blocks) {
      if (block.error || block.frame !== next || block.sequence !== sequence
          || block.left?.length !== 128 || block.right?.length !== 128) {
        metadata.error ??= 'missing or duplicate sample graph quantum at ' + next;
      }
      next += 128; sequence++;
    }
    if (!(end > start) || next !== end) metadata.error ??= 'incomplete sample graph window';
    if (metadata.error) throw Error('sample graph rejected: ' + metadata.error);
    const pcm = new Float32Array((end - start) * 2);
    for (let j = 0; j < blocks.length; j++) for (let i = 0; i < 128; i++) {
      pcm[j * 256 + i * 2] = blocks[j].left[i];
      pcm[j * 256 + i * 2 + 1] = blocks[j].right[i];
    }
    probe.capturedPcm = pcm;
    return metadata;
  };
  probe.match = (start, end, shift = 0) => {
    if (probe.error) throw Error(`sample graph rejected: ${probe.error}`);
    if (probe.buffers.some(b => b.admission.kind === 'immediate' && b.frame < end
        && (b.admission.endedFrame === null || b.admission.endedFrame > start))) {
      throw Error('sample graph model rejected: immediate source lacks render-generation admission');
    }
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
