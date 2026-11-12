// Install after session-sample-graph.js with __sessionSampleClockOutput enabled.
// Each source has an independent silent diagnostic branch. No command timestamp
// is promoted to a generation boundary, and no missing quantum is synthesized.
(() => {
  const probe = globalThis.__sourceGeneration = {sources: [], error: null};
  const nativeStart = AudioBufferSourceNode.prototype.start;
  const connect = AudioNode.prototype.connect;
  let context, ready;
  const drains = new Map();
  const module = URL.createObjectURL(new Blob([`
    registerProcessor('source-generation-tap', class extends AudioWorkletProcessor {
      constructor(options) {
        super(); this.id = options.processorOptions.id; this.endpoint = Infinity;
        this.port.onmessage = ({data}) => { this.endpoint = data.endpoint; };
      }
      process(inputs) {
        const [signal, clock] = inputs;
        if (clock[0]?.length !== 128 || clock[1]?.length !== 128) return true;
        const frame = clock[0][0], sequence = clock[1][0];
        // The shared clock output is silent until its explicitly armed interval.
        if (!frame) return true;
        const error = clock[0].some((v, i) => v !== frame + i)
          || clock[1].some(v => v !== sequence);
        if (frame >= this.endpoint) {
          this.port.postMessage({endpointAck:true, frame, error});
          return false;
        }
        if (!this.marked) { console.timeStamp('source-generation:' + this.id + ':' + frame); this.marked = true; }
        const left = signal[0] ?? new Float32Array(128), right = signal[1] ?? new Float32Array(128);
        const zero = left.every(v => v === 0) && right.every(v => v === 0);
        this.port.postMessage({frame, sequence, error, zero,
          ...(zero ? {} : {left, right})});
        return true;
      }
    });
  `], {type: 'text/javascript'}));
  probe.freeze = async end => {
    if (probe.error) throw Error(probe.error);
    const sources = probe.sources.slice();
    // Port delivery is independent for each tap. Fix the endpoint first, then
    // drain every port through its render-thread acknowledgement. Earlier PCM
    // messages on that port must arrive before its acknowledgement.
    for (const source of sources) if (source.observed) {
      const drain = drains.get(source.id);drain.end = end;drain.tap.port.postMessage({endpoint:end});
    }
    let timer;
    try {
      await Promise.race([
        Promise.all(sources.filter(s => s.observed).map(s => drains.get(s.id).done)),
        new Promise((_, reject) => {timer = setTimeout(() => reject(Error('source endpoint drain deadline')),30000);}),
      ]);
    } finally {clearTimeout(timer);}
    if (probe.error) throw Error(probe.error);
    probe.captured = structuredClone({error: probe.error, sources: sources.map(s => ({...s,
      blocks:s.blocks.filter(b => b.frame < end),
      retiredFrame: globalThis.__sessionSamples.retirements[s.id] ?? null}))});
    console.timeStamp('source-generation-endpoint');
    return {sources: probe.captured.sources.length};
  };
  AudioBufferSourceNode.prototype.start = function(...args) {
    try {
      if ((!globalThis.__sessionSampleSourceFilter || globalThis.__sessionSampleSourceFilter(this))
          && !this.loop && this.buffer?.numberOfChannels === 2) {
        if (context && context !== this.context) throw Error('multiple source generation contexts');
        context = this.context;
        if (!ready) ready = context.audioWorklet.addModule(module).then(() => {probe.ready = true;})
          .catch(error => {probe.error = String(error);});
        const source = {id: probe.sources.length, observed: false, blocks: [],
          requested: args[0] ?? 0, offset: args[1] ?? 0, duration: args[2] ?? null,
          rate: this.buffer.sampleRate, frames: this.buffer.length};
        this.addEventListener('ended', () => {
          try { globalThis.__sessionSamples.retireSource(source.id); }
          catch (error) { probe.error = String(error); }
        }, {once: true});
        probe.sources.push(source);
        if (probe.ready && globalThis.__sessionSamples?.status().ready) {
          const tap = new AudioWorkletNode(context, 'source-generation-tap', {
            numberOfInputs: 2, channelCount: 2, channelCountMode: 'explicit',
            outputChannelCount: [2], processorOptions: {id: source.id}});
          connect.call(this, tap, 0, 0);
          __sessionSamples.connectClock(tap, 1);
          connect.call(tap, context.destination);
          source.observed = true;
          let finish;
          const done = new Promise(resolve => {finish = resolve;});
          drains.set(source.id, {tap, done});
          tap.port.onmessage = ({data}) => {
            if (data.endpointAck) {
              if (data.error || !Number.isInteger(data.frame) || data.frame < drains.get(source.id).end)
                probe.error ??= 'invalid source endpoint acknowledgement';
              tap.disconnect();finish();return;
            }
            const previous = source.blocks.at(-1);
            if (data.error || previous && (data.frame !== previous.frame + 128
                || data.sequence !== previous.sequence + 1)) {
              probe.error ??= 'discontinuous source generation quantum: ' + source.id;
            }
            if (source.blocks.length >= 2 ** 24 / 128) probe.error ??= 'source generation capacity exceeded';
            else source.blocks.push(data);
          };
        }
        console.timeStamp('source-command:' + source.id);
      }
    } catch (error) { probe.error = String(error); }
    // Observer failures never suppress activation or replace native exceptions.
    return nativeStart.apply(this, args);
  };
})();
