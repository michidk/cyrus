import assert from 'node:assert/strict';

// Native evidence must cover the armed interval, including the last PCM quantum.
// Missing trace events are never repaired from command times or worklet globals.
export function requireCinematicTrace(trace, capture, completion, endpoint = 'source-generation-endpoint') {
  assert(completion && completion.dataLossOccurred === false, 'native trace lost events or lacks completion');
  const counts = new Map();
  for (const event of trace) if (event.name === 'TimeStamp') {
    const name = event.args?.data?.message;
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  const one = name => assert.equal(counts.get(name), 1, `native trace missing or duplicate boundary: ${name}`);
  one('cinematic-playback-armed');
  one(endpoint);
  for (const block of capture.graph.blocks) one('session-generation:' + block.frame);
  for (let i = 0; i < (capture.seeks ?? []).length; i++) {
    one('cinematic-seek-begin:' + i);
    one('cinematic-seek-end:' + i);
  }
  for (const source of capture.sources) {
    one('source-command:' + source.id);
    if (source.stopRequested !== undefined) one('source-stop:' + source.id);
    if (Number.isInteger(source.drainedFrame)) one(`source-drained:${source.id}:${source.drainedFrame}`);
    if (Number.isInteger(source.retiredFrame)) one(`source-retired:${source.id}:${source.retiredFrame}`);
  }
  return {events:trace.length, sourceCommands:capture.sources.length, sampleQuanta:capture.graph.blocks.length};
}

// A public seek with an empty bank cannot prove cancellation of queued audio.
// Require native stop and acknowledged retirement for the identities owned at
// dispatch, then reject any old PCM at or after the first new native processing.
export function requireCinematicCancellation(capture, boundaries) {
  assert(capture.seeks?.length, 'no production seek dispatch observed');
  let cancelled = 0;
  for (const seek of capture.seeks) {
    assert(seek.active?.length, 'seek had no active sources; queued cancellation remains uncovered');
    assert(Number.isFinite(seek.contextTime), 'seek lacks actual audio context clock');
    assert(Number.isInteger(seek.sourceCount), 'seek lacks source creation boundary');
    const next = capture.sources.filter(s => s.id >= seek.sourceCount);
    const adopted = boundaries.filter(b => b.attributed && next.some(s => s.id === b.id));
    assert(adopted.length, 'seek continuation has no native first-processing evidence');
    const first = Math.min(...adopted.map(b => b.firstProcessingFrame));
    for (const id of seek.active) {
      const source = capture.sources.find(s => s.id === id);
      assert(source && Number.isFinite(source.stopRequested) && source.stopRequested >= seek.contextTime
        && source.stopRequested <= seek.endContextTime, 'active source lacks seek stop boundary');
      assert(Number.isInteger(source.retiredFrame) && source.retiredFrame <= first,
        'active source lacks retirement before continuation');
      assert(!source.blocks.some(b => b.frame >= first && !b.zero &&
        [...b.left, ...b.right].some(value => value !== 0)), 'obsolete source PCM survived seek continuation');
      cancelled++;
    }
  }
  return {seeks:capture.seeks.length, cancelled};
}

// This receipt is finalized before releasing destination decoding. It proves
// retirement of actual owners without requiring a later movie frame to exist.
export function requireCancellationPhase(capture, boundaries) {
  assert.equal(capture.seeks?.length,1,'cancellation phase requires one actual public seek');
  const seek=capture.seeks[0];
  assert(seek.active?.length,'seek had no active sources; queued cancellation remains uncovered');
  const receipt=capture.phase?.retirement;
  assert(capture.phase?.state==='retired' && receipt?.length===seek.active.length,'incomplete cancellation phase');
  for(const id of seek.active) {
    const source=capture.sources.find(s=>s.id===id), boundary=boundaries.find(b=>b.id===id);
    const retired=receipt.find(r=>r.id===id);
    assert(boundary?.attributed,'cancelled owner lacks native processing attribution');
    assert(source && Number.isFinite(source.stopRequested) && source.stopRequested>=seek.contextTime
      && source.stopRequested<=seek.endContextTime,'active source lacks seek stop boundary');
    assert(retired && retired.frame===source.retiredFrame && retired.end===source.drainedFrame
      && retired.end>=retired.frame,'cancelled owner lacks exact render/drain acknowledgement');
    assert(!source.blocks.some(b=>b.frame>=retired.frame && !b.zero &&
      [...b.left,...b.right].some(v=>v!==0)),'obsolete source PCM survived acknowledged retirement');
  }
  return {cancelled:seek.active,retirement:receipt};
}
