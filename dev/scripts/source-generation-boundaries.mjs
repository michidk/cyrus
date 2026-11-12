import assert from 'node:assert/strict';

const contains = (outer, inner) => outer.pid === inner.pid && outer.tid === inner.tid
  && outer.ts <= inner.ts && outer.ts + outer.dur >= inner.ts + (inner.dur ?? 0);

// Trace addresses are ephemeral identities, never persisted in application code.
// A source-owned tap names its direct input even when the shared mixer pulled
// that source earlier. Descendants pulled through the clock bus are excluded.
export function sourceGenerationBoundaries(trace, sources) {
  const handlers = trace.filter(e => e.name === 'AudioHandler::ProcessIfNecessary');
  const stamps = new Map();
  for (const e of trace) if (e.name === 'TimeStamp') {
    const name = e.args?.data?.message;
    if (!stamps.has(name)) stamps.set(name, []);
    stamps.get(name).push(e);
  }
  const processes = trace.filter(e => e.name === 'AudioBufferSourceHandler::Process')
    .sort((a, b) => a.ts - b.ts);
  const firstProcess = pull => {
    let lo = 0, hi = processes.length;
    while (lo < hi) { const mid = (lo + hi) >>> 1;
      if (processes[mid].ts < pull.ts) lo = mid + 1; else hi = mid;
    }
    for (let i = lo; i < processes.length && processes[i].ts <= pull.ts + pull.dur; i++) {
      if (contains(pull, processes[i])) return processes[i];
    }
  };
  return sources.filter(s => s.observed).map(source => {
    assert(source.blocks.length, 'source has no observed generation quanta');
    let previous;
    const identities = new Set(), admissions = [];
    const attributionBlock=source.blocks.find(b=>b.connected===true)??source.blocks[0];
    for (const block of source.blocks) {
      assert(!block.error && (block.zero === true || block.left?.length === 128 && block.right?.length === 128),
        'invalid source generation quantum');
      assert(!previous || block.frame === previous.frame + 128 && block.sequence === previous.sequence + 1,
        'discontinuous source generation quantum');
      previous = block;
      if (block !== attributionBlock) continue;
      const markers = stamps.get(`source-generation:${source.id}:${block.frame}`) ?? [];
      assert.equal(markers.length, 1, 'require one source generation marker');
      const parents = handlers.filter(e => e.args?.['node type'] === 'AudioWorkletNode' && contains(e, markers[0]));
      const parent = parents.sort((a, b) => a.dur - b.dur)[0];
      assert(parent, 'generation marker has no native tap owner');
      const children = handlers.filter(e => e !== parent && contains(parent, e));
      const direct = children.filter(e => e.args?.['node type'] === 'AudioBufferSourceNode'
        && !children.some(other => other !== e && contains(other, e)));
      // An ended source may be removed from the input. Its observed zero block
      // remains part of continuity, but cannot supply a new ownership identity.
      assert(direct.length <= 1, 'ambiguous direct source input');
      if (direct.length) {
        identities.add(direct[0].args.this);
        admissions.push({frame:block.frame, sequence:block.sequence, native:direct[0]});
      }
    }
    if (identities.size === 0 && Number.isInteger(source.retiredFrame)
        && source.blocks.every(b => b.zero === true)) {
      // Cancellation can remove the input before the diagnostic branch joins.
      // This is NOT positive first-processing evidence. A caller may compare
      // only a later interval after the acknowledged native retirement.
      return {id:source.id, attributed:false, retiredFrame:source.retiredFrame,
        reason:'retired source has no observed native input identity'};
    }
    assert.equal(identities.size, 1, 'require one native source identity');
    const identity = [...identities][0];
    const commands = stamps.get(`source-command:${source.id}`) ?? [];
    assert.equal(commands.length, 1, 'require one source activation bracket');
    const pulls = handlers.filter(e => e.args?.this === identity && e.pid === commands[0].pid && e.ts >= commands[0].ts).sort((a, b) => a.ts - b.ts);
    const processing = pulls.map(firstProcess).filter(Boolean);
    assert(processing.length, 'source never processed in the native rendering graph');
    const first = admissions[0];
    const render = trace.filter(e => e.name === 'RealtimeAudioDestinationHandler::Render'
      && contains(e, processing[0])).sort((a, b) => a.dur - b.dur)[0];
    assert(render, 'first source processing has no native render quantum');
    const clocks = new Set([...stamps.entries()].filter(([name]) => /^session-generation:\d+$/.test(name))
      .flatMap(([name, events]) => events.filter(e => contains(render, e)).map(() => Number(name.split(':')[1]))));
    assert.equal(clocks.size, 1, `native render quantum lacks one exact generation clock: source ${source.id}, process ${processing[0].ts}, render ${render.ts}..${render.ts + render.dur}`);
    const firstProcessingFrame = [...clocks][0];
    return {id:source.id, attributed:true, firstObservedFrame:source.blocks[0].frame,
      firstInputFrame:first.frame, firstInputTraceTime:first.native.ts,
      firstProcessingTraceTime:processing[0].ts, firstProcessingFrame,
      missingPrefixFrames:Math.max(0, source.blocks[0].frame-firstProcessingFrame),
      lastObservedFrame:source.blocks.at(-1).frame, quanta:source.blocks.length};
  });
}
