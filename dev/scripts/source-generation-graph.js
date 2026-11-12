// Install after session-sample-graph.js with __sessionSampleClockOutput enabled.
// Each source has an independent silent diagnostic branch. No command timestamp
// is promoted to a generation boundary, and no missing quantum is synthesized.
(() => {
  const probe = globalThis.__sourceGeneration = {sources: [], error: null};
  const nativeStart = AudioBufferSourceNode.prototype.start;
  const connect = AudioNode.prototype.connect;
  let context, ready;
  const drains = new Map(), identities = new WeakMap();
  probe.idFor = source => identities.get(source)?.id;
  probe.arm = candidate => {
    if (context && context !== candidate) throw Error('multiple source generation contexts');
    context = candidate;
    if (!ready) ready = context.audioWorklet.addModule(module).then(() => {probe.ready = true;});
    return ready;
  };
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
          if (!this.acknowledged) {
            console.timeStamp('source-drained:' + this.id + ':' + frame);
            this.port.postMessage({endpointAck:true, frame, error});this.acknowledged=true;
          }
          return false;
        }
        const connected=signal[0]?.length===128;
        if (!this.marked || connected && !this.connectedMarked) {
          console.timeStamp('source-generation:' + this.id + ':' + frame);this.marked=true;
          if(connected)this.connectedMarked=true;
        }
        const left = signal[0] ?? new Float32Array(128), right = signal[1] ?? new Float32Array(128);
        const zero = left.every(v => v === 0) && right.every(v => v === 0);
        this.port.postMessage({frame, sequence, reportedFrame:currentFrame, error, zero, connected,
          ...(zero ? {} : {left, right})});
        return true;
      }
    });
  `], {type: 'text/javascript'}));
  probe.retirement = async ids => {
    const owned = ids.map(id => {
      const drain = drains.get(id);
      if (!drain) throw Error('retiring source lacks observed graph ownership: ' + id);
      return {id,drain};
    });
    await Promise.all(owned.map(({drain}) => drain.done));
    if (probe.error) throw Error(probe.error);
    return owned.map(({id,drain}) => {
      const frame = globalThis.__sessionSamples.retirements[id];
      if (!Number.isInteger(frame) || !Number.isInteger(drain.ack)) throw Error('source retirement lacks render acknowledgement');
      return {id,frame,end:drain.ack};
    });
  };
  probe.freeze = async end => {
    if (probe.error) throw Error(probe.error);
    const sources = probe.sources.slice();
    // Port delivery is independent for each tap. Fix the endpoint first, then
    // drain every port through its render-thread acknowledgement. Earlier PCM
    // messages on that port must arrive before its acknowledgement.
    for (const source of sources) if (source.observed) {
      const drain = drains.get(source.id);if (!drain.closed) {drain.end = Math.min(drain.end ?? Infinity,end);drain.tap.port.postMessage({endpoint:drain.end});}
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
        probe.arm(this.context).catch(error => {probe.error = String(error);});
        const source = {id: probe.sources.length, observed: false, blocks: [],
          requested: args[0] ?? 0, offset: args[1] ?? 0, duration: args[2] ?? null,
          rate: this.buffer.sampleRate, frames: this.buffer.length};
        this.addEventListener('ended', () => {
          try {
            const retirement = globalThis.__sessionSamples.retireSource(source.id);
            if (globalThis.__cinematicRetireTaps) Promise.resolve(retirement).then(frame => {
              const drain = drains.get(source.id);
              if (drain && Number.isInteger(frame)) {
                drain.end = Math.min(drain.end ?? Infinity,frame + 128); drain.tap.port.postMessage({endpoint:drain.end});
              }
            }).catch(error => {probe.error = String(error);});
          }
          catch (error) { probe.error = String(error); }
        }, {once: true});
        probe.sources.push(source); identities.set(this, source);
        if (probe.ready && globalThis.__sessionSamples?.status().ready) {
          const tap = new AudioWorkletNode(context, 'source-generation-tap', {
            numberOfInputs: 2, channelCount: 2, channelCountMode: 'explicit',
            outputChannelCount: [2], processorOptions: {id: source.id}});
          connect.call(this, tap, 0, 0);
          __sessionSamples.connectClock(tap, 1);
          if(globalThis.__cinematicOrderedTaps)__sessionSamples.connectObserver(tap);
          else connect.call(tap, context.destination);
          source.observed = true;
          let finish;
          const done = new Promise(resolve => {finish = resolve;});
          drains.set(source.id, {tap, done});
          tap.port.onmessage = ({data}) => {
            if (data.endpointAck) {
              if (data.error || !Number.isInteger(data.frame) || data.frame < drains.get(source.id).end)
                probe.error ??= 'invalid source endpoint acknowledgement';
              drains.get(source.id).closed = true;drains.get(source.id).ack = data.frame;source.drainedFrame=data.frame;
              tap.disconnect();finish();return;
            }
            const previous = source.blocks.at(-1);
            if (data.error || previous && (data.frame !== previous.frame + 128
                || data.sequence !== previous.sequence + 1)) {
              probe.error ??= 'discontinuous source generation quantum: ' + source.id + ', previous ' + JSON.stringify({frame:previous?.frame,sequence:previous?.sequence,reportedFrame:previous?.reportedFrame}) + ', actual ' + JSON.stringify({frame:data.frame,sequence:data.sequence,reportedFrame:data.reportedFrame,error:data.error});
            }
            if (source.blocks.length >= 2 ** 24 / 128) probe.error ??= 'source generation capacity exceeded';
            else source.blocks.push(data);
            try {globalThis.__cinematicSeekGate?.generation();}catch(error){probe.error??=String(error);}
          };
        }
        console.timeStamp('source-command:' + source.id);
      }
    } catch (error) { probe.error = String(error); }
    // Observer failures never suppress activation or replace native exceptions.
    return nativeStart.apply(this, args);
  };
  const nativeStop = AudioBufferSourceNode.prototype.stop;
  AudioBufferSourceNode.prototype.stop = function(...args) {
    try {
      const source = identities.get(this);
      if (source) { source.stopRequested = this.context.currentTime; console.timeStamp('source-stop:' + source.id); }
    } catch (error) { probe.error = String(error); }
    return nativeStop.apply(this, args);
  };

})();
