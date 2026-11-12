// Diagnostic-only audio-clock hold bridges public-input transport latency. It
// never supplies evidence of normal playback cadence or replaces native seek.
export function installCinematicSeekGate(minimumFrame) {
  const gate = globalThis.__cinematicSeekGate = {state:'waiting',history:[]};
  let audio, releaseResume;
  const release=()=>{const resume=releaseResume;releaseResume=null;resume?.();};
  const fail=error=>{release();gate.state='failed';globalThis.__cinematicClockError=String(error);};
  const owners = candidate => candidate.sources.map(node => {
    const id = globalThis.__sourceGeneration.idFor(node);
    const source = globalThis.__sourceGeneration.sources[id];
    if (!source || !Number.isFinite(source.duration)) throw Error('queued source lacks explicit playback extent');
    return {id, end:source.requested + source.duration, processed:source.blocks?.some(b=>b.connected===true)??false};
  }).filter(source => source.end > candidate.context.currentTime);
  const snapshot = event => {
    const state={event,contextTime:audio?.context.currentTime,contextState:audio?.context.state,
      paused:audio?.paused,position:audio?.position,origin:audio?.origin,videoEnd:audio?.videoEnd,
      bank:audio?.sources.map(node=>globalThis.__sourceGeneration.idFor(node))};
    gate.history.push(state);return state;
  };
  gate.beforeResume = candidate => {if(candidate===audio)snapshot('production-resume');};
  gate.consider = candidate => {
    const frame = globalThis.__cyrusMenu?.movie?.frame;
    if (gate.state !== 'waiting' || !Number.isInteger(frame) || frame < minimumFrame) return;
    audio=candidate;
    if (!owners(candidate).some(owner=>owner.processed)) return;
    gate.state = 'holding';audio = candidate;snapshot('hold-request');
    const context=audio.context, resume=context.resume, descriptor=Object.getOwnPropertyDescriptor(context,'resume');
    const pending=[];
    context.resume=function(...args) {
      snapshot('held-resume-request');
      return new Promise((resolve,reject)=>pending.push({receiver:this,args,resolve,reject}));
    };
    releaseResume=()=>{
      if(descriptor)Object.defineProperty(context,'resume',descriptor);else delete context.resume;
      for(const call of pending) {
        try {Promise.resolve(resume.apply(call.receiver,call.args)).then(call.resolve,call.reject);}
        catch(error){call.reject(error);}
      }
    };
    Promise.resolve(candidate.context.suspend()).then(() => {
      const active = owners(candidate);
      if (candidate.context.state !== 'suspended' || !active.length) throw Error('source ownership expired before clock hold');
      gate.trigger = {active, contextTime:candidate.context.currentTime, frame:globalThis.__cyrusMenu.movie.frame};
      gate.state = 'ready';snapshot('hold-ack');
    }).catch(fail);
  };
  gate.generation=()=>{if(audio)gate.consider(audio);};
  gate.afterSeek = boundary => {
    snapshot('seek-return');gate.seek=boundary;
    if (gate.state !== 'ready') {release();throw Error('public seek arrived without acknowledged active-bank hold');}
    if (!gate.trigger.active.some(source => boundary.active.includes(source.id))) {release();throw Error('trigger ownership lost before public seek dispatch');}
    gate.state = 'retiring';release();
    globalThis.__cinematicPhaseHeld = true;
    // resume drives the real native stops and rendering retirement acknowledgements.
    Promise.resolve(audio.context.resume()).then(() => globalThis.__sourceGeneration.retirement(boundary.active))
      .then(receipt => {gate.retirement=receipt;gate.state='retired';console.timeStamp('cinematic-cancellation-phase');})
      .catch(fail);
  };
  gate.status = () => ({state:gate.state,trigger:gate.trigger,retirement:gate.retirement,history:gate.history,seek:gate.seek,live:audio?{contextState:audio.context.state,contextTime:audio.context.currentTime,bank:audio.sources.map(node=>globalThis.__sourceGeneration.idFor(node))}:null});
}
