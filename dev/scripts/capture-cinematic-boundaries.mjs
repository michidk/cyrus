// Bounded installed production recording. All generated evidence stays outside git.
import assert from 'node:assert/strict';
import {promisify} from 'node:util';
import {execFile} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {setTimeout as wait} from 'node:timers/promises';
import {join} from 'node:path';
import {observeBrowserBuild} from './browser-provenance.mjs';
import {writeJsonArray} from './write-json-array.mjs';
import {connectCdp} from './capture-cdp.mjs';
import {compareSourceGeneration} from './compare-source-generation.mjs';
import {navigateCinematic, installCinematicEndpoint, observeMovieTime, observeMovieSchedule, requireMovieHorizon, armCinematicPlayback, observeMovieSeek, cinematicProgress} from './cinematic-navigation.mjs';
import {requireCinematicTrace, requireCinematicCancellation, requireCancellationPhase} from './cinematic-trace.mjs';
import {installCinematicSeekGate} from './cinematic-seek-gate.mjs';
import {installCinematicComposition} from './cinematic-composition.mjs';
import {sourceGenerationBoundaries} from './source-generation-boundaries.mjs';

const [endpoint, page, output, frameArgument, rateArgument, skipArgument] = process.argv.slice(2);
const stopFrame = Number(frameArgument), audioRate = Number(rateArgument), skipAt = Number(skipArgument ?? 0);
assert(Number.isInteger(skipAt) && skipAt >= 0 && (!skipAt || stopFrame > skipAt), 'skip frame must precede endpoint');
assert(endpoint && page && output && Number.isInteger(stopFrame) && stopFrame > 0
  && Number.isInteger(audioRate) && audioRate > 0,
'usage: CDP_URL EXPLICIT_INSTALL_MOVIE_URL NEW_OUTPUT_DIRECTORY MIN_END_FRAME INSTALLED_AUDIO_RATE');
// Presentation can reuse an independently admitted cancellation phase without
// suspending its new audio graph or relabelling its seek as cancellation coverage.
const acceptedCancellation=process.env.CYRUS_CINEMATIC_ACCEPTED_CANCELLATION;
let reusedCancellation;
if(acceptedCancellation) {
  assert(skipAt,'cancellation receipt reuse requires a public presentation seek');
  const phaseBytes=readFileSync(join(acceptedCancellation,'cancellation-phase.json'));
  const boundaryBytes=readFileSync(join(acceptedCancellation,'cancellation-boundaries.json'));
  const phase=JSON.parse(phaseBytes), boundaries=JSON.parse(boundaryBytes);
  const traceBytes=readFileSync(join(acceptedCancellation,'cancellation-trace.json'));
  const trace=JSON.parse(traceBytes), completion=JSON.parse(readFileSync(join(acceptedCancellation,'cancellation-trace-completion.json')));
  requireCinematicTrace(trace,phase,completion,'cinematic-cancellation-phase');
  assert.deepEqual(boundaries,sourceGenerationBoundaries(trace,phase.sources),'retained native boundaries differ from original trace');
  reusedCancellation={traceSha256:createHash('sha256').update(traceBytes).digest('hex'),phaseSha256:createHash('sha256').update(phaseBytes).digest('hex'),
    boundariesSha256:createHash('sha256').update(boundaryBytes).digest('hex'),
    result:requireCancellationPhase(phase,boundaries),pcm:compareSourceGeneration(phase,boundaries)};
}
const controlledCancellation=skipAt && !reusedCancellation;
const presentedWindow=process.env.CYRUS_CINEMATIC_PRESENTED_WINDOW;
assert(!skipAt || presentedWindow,'public seek acceptance requires an independently exported presented-frame window');
const presentedMaximum=presentedWindow ? Math.max(...JSON.parse(readFileSync(join(presentedWindow,'window.json'),'utf8')).frames.map(frame=>frame.number)) : Infinity;
assert(stopFrame<=presentedMaximum,'requested endpoint is outside the independent reference window');
assert(process.env.CYRUS_BROWSER_BUILD_RECORD, 'set CYRUS_BROWSER_BUILD_RECORD to the sealed build receipt');
const build = JSON.parse(readFileSync(process.env.CYRUS_BROWSER_BUILD_RECORD, 'utf8'));
assert(!existsSync(output), 'use a new output directory; never overwrite a retained recording');
mkdirSync(output, {recursive:true});
if(reusedCancellation)writeFileSync(join(output,'reused-cancellation.json'),JSON.stringify(reusedCancellation));
const cdp = await connectCdp(endpoint), trace = [], surfaces = [];
let traceDone, traceEnding = false, tracing = false, recording = false, failure = null, provenance, traceCompletion;
let ended;
async function startTrace() {
  traceEnding=false;traceCompletion=undefined;
  ended=new Promise(resolve=>{traceDone=resolve;});
  await cdp.call('Tracing.start',{categories:'disabled-by-default-webaudio.audionode,webaudio,devtools.timeline',transferMode:'ReportEvents'},null);
  tracing=true;
}
async function saveCancellationPhase() {
  const capture=await evaluate(`({phase:__cinematicSeekGate.status(),seeks:__cinematicSeeks,
    sources:__sourceGeneration.sources.map(s=>({...s,retiredFrame:__sessionSamples.retirements[s.id]??null,
      blocks:s.blocks.map(b=>({...b,...(b.zero?{}:{left:Array.from(b.left),right:Array.from(b.right)})}))})),
    graph:{rate:__sessionSamples.clock().rate,blocks:__sessionSamples.blocks.slice(-200).map(b=>({...b,left:Array.from(b.left),right:Array.from(b.right)}))}})`);
  await endTrace();
  writeFileSync(join(output,'cancellation-phase.json'),JSON.stringify(capture));
  writeFileSync(join(output,'cancellation-trace.json'),JSON.stringify(trace));
  writeFileSync(join(output,'cancellation-trace-completion.json'),JSON.stringify(traceCompletion));
  requireCinematicTrace(trace,capture,traceCompletion,'cinematic-cancellation-phase');
  const boundaries=sourceGenerationBoundaries(trace,capture.sources);
  writeFileSync(join(output,'cancellation-boundaries.json'),JSON.stringify(boundaries));
  const result=requireCancellationPhase(capture,boundaries);
  const pcm=compareSourceGeneration(capture,boundaries);
  writeFileSync(join(output,'cancellation-phase-result.json'),JSON.stringify({result,pcm}));
  // Only an admitted, durable phase may release the held production animation.
  await startTrace();
  await evaluate('__releaseCinematicPhase()');
}
async function evaluate(expression) {
  const r = await cdp.call('Runtime.evaluate', {expression, returnByValue:true, awaitPromise:true});
  assert(!r.exceptionDetails, r.result.description ?? r.exceptionDetails?.text);
  return r.result.value;
}
async function endTrace() {
  if (!traceEnding) {traceEnding = true;await cdp.call('Tracing.end', {}, null);}
  let timer;
  try { await Promise.race([ended, new Promise((_, reject) => {
    timer = setTimeout(() => reject(Error('trace completion deadline')), 120000);
  })]); } finally {clearTimeout(timer);}
  tracing = false;
}
cdp.onEvent(message => {
  if (message.method === 'Tracing.dataCollected') trace.push(...message.params.value);
  if (message.method === 'Tracing.tracingComplete') {traceCompletion=message.params;traceDone();}
  if (message.method === 'Page.screencastFrame') {
    const f = message.params, id = surfaces.length;
    writeFileSync(join(output, `surface-${id}.png`), Buffer.from(f.data, 'base64'));
    surfaces.push({id, arrivalSequence:id, receivedMonotonicMs:performance.now(), metadata:f.metadata});
    cdp.call('Page.screencastFrameAck', {sessionId:f.sessionId})
      .catch(error => {failure ??= String(error);});
  }
});
try {
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride', {width:640,height:480,deviceScaleFactor:1,mobile:false});
  provenance = await observeBrowserBuild(cdp, build, page);
  // Observe calls made by production. Extra getter polls can themselves pause
  // an exhausted packet stream and would change the behavior being measured.
  await cdp.call('Debugger.setBreakpointByUrl', {urlRegex:'movie_stream\\.js$',lineNumber:104,
    condition:`(()=>{if(!window.__movieTimeObserved){window.__movieTimeObserved=true;
      seekMovie=(${observeMovieSeek.toString()})(seekMovie,id=>movies.get(id),window.__cinematicSeeks=[]);
      PacketAudio.prototype.play=(${armCinematicPlayback.toString()})(PacketAudio.prototype.play);
      PacketAudio.prototype.schedule=(${observeMovieSchedule.toString()})(PacketAudio.prototype.schedule);
      movieTime=(${observeMovieTime.toString()})(movieTime,id=>movies.get(id),window.__returnedMovieClocks=[]);}
      return false;})()`});
  const graph = '(' + installCinematicComposition.toString() + ')();' + '(' + installCinematicEndpoint.toString() + ')();'
    + (controlledCancellation ? '(' + installCinematicSeekGate.toString() + ')(' + skipAt + ');' : '') + `globalThis.__sessionSampleClockOutput=true;globalThis.__cinematicRetireTaps=true;globalThis.__cinematicOrderedTaps=true;
    globalThis.__sessionSampleSourceFilter=s=>s.buffer?.sampleRate===${audioRate};`
    + readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8')
    + readFileSync(new URL('./source-generation-graph.js', import.meta.url), 'utf8');
  await cdp.call('Page.addScriptToEvaluateOnNewDocument', {source:graph});
  await startTrace();
  await navigateCinematic(cdp, page);
  await cdp.call('Page.bringToFront');
  const deadline = performance.now() + 240000;
  let debuggerReleased = false, skipSent = false;
  let reached = false, phaseSaved = false;
  while (performance.now() < deadline) {
    const state = await evaluate(`(${cinematicProgress.toString()})(${stopFrame})`);
    writeFileSync(join(output, 'progress.json'), JSON.stringify(state));
    if(state.movie?.frame>=1 && !existsSync(join(output,'movie-composition.json'))) {
      const composition=await evaluate(`({metadata:__cinematicMovieMetadata[new URL(location.href).searchParams.get('movie')??'0'],preferences:__cyrusMenu.preferences})`);
      assert(composition.metadata && composition.preferences,'missing installed movie composition/settings');
      writeFileSync(join(output,'movie-composition.json'),JSON.stringify(composition));
    }
    assert(!failure && !state.error && !state.observer && !state.graph,
      failure ?? state.error ?? state.observer ?? state.graph);
    if (state.movie?.frame >= 1 && !debuggerReleased) {
      writeFileSync(join(output,'executed-build.json'),JSON.stringify(await provenance.verify(),null,2));
      await cdp.call('Debugger.disable');debuggerReleased = true;
    }
    if (state.movie?.frame >= 1 && !recording) {
      // Bind to the established movie compositor, rather than the loading page's
      // surface. A loading-page screencast can stop delivering at layer replacement.
      await cdp.call('Page.startScreencast', {format:'png',maxWidth:640,maxHeight:480,everyNthFrame:1});
      recording = true;
      await cdp.call('Input.dispatchMouseEvent', {type:'mousePressed',x:320,y:240,button:'left',clickCount:1});
      await cdp.call('Input.dispatchMouseEvent', {type:'mouseReleased',x:320,y:240,button:'left',clickCount:1});
    }
    if (skipAt && !skipSent && (controlledCancellation ? state.seekGate?.state === 'ready' : state.movie?.frame >= skipAt && !state.movie.loading)) {
      await evaluate(`console.timeStamp('cinematic-public-skip')`);
      await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await wait(100);
      await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      skipSent = true;
      writeFileSync(join(output,'skip.json'),JSON.stringify({from:state.movie.frame,key:'Escape'}));
    }
    if (controlledCancellation && state.seekGate?.state === 'retired' && !phaseSaved) {
      await saveCancellationPhase();phaseSaved=true;
    }
    if (state.movie?.frame >= stopFrame && !state.movie.loading && state.signalEnergy > 0.001) {
      if(presentedWindow) {
        // Force pending GPU work to complete, then admit only independently
        // identified compositor swaps. Published menu status is not presentation.
        const shot=await cdp.call('Page.captureScreenshot',{format:'png',fromSurface:true});
        writeFileSync(join(output,'endpoint.png'),Buffer.from(shot.data,'base64'));
        writeFileSync(join(output,'surfaces.json'),JSON.stringify(surfaces));
        const composition=await evaluate(`({metadata:__cinematicMovieMetadata[new URL(location.href).searchParams.get('movie')??'0'],preferences:__cyrusMenu.preferences})`);
        assert(composition.metadata && composition.preferences,'missing installed movie composition/settings');
        writeFileSync(join(output,'movie-composition.json'),JSON.stringify(composition));
        // Oracle composition is offline work. Freeze the admitted endpoint and
        // drain tracing first, so analysis cannot extend PCM or overflow tracing.
      }
      reached = true;break;
    }
    await wait(100);
  }
  assert(reached, 'movie progression endpoint deadline');
  assert(!controlledCancellation || phaseSaved,'public seek lacks persisted cancellation phase');
  // Freeze source, mixed PCM and clock observations in one task before readback.
  await evaluate(`(async()=>{globalThis.__cinematicEndpointStopped=true;const p=__sessionSamples,windowBlocks=p.blocks.slice(-200);
    const drain=__sourceGeneration.freeze(windowBlocks.at(-1)?.frame+128);
    p.capture(windowBlocks[0]?.frame,windowBlocks.at(-1)?.frame+128);window.__boundaryGraph=structuredClone({
    error:__sessionSamples.error,rate:__sessionSamples.clock().rate,
    blocks:__sessionSamples.blocks.slice(-200),returnedClockReads:window.__returnedMovieClocks,
    movie:window.__cyrusMenu?.movie});await drain;return true;})()`);
  await cdp.call('Page.stopScreencast');recording = false;
  await endTrace();
  const capture = await evaluate(`({seeks:window.__cinematicSeeks,sources:__sourceGeneration.captured.sources.map(s=>({...s,
    blocks:s.blocks.map(b=>({...b,...(b.zero?{}:{left:Array.from(b.left),right:Array.from(b.right)})}))})),
    graph:{...__boundaryGraph,blocks:__boundaryGraph.blocks.map(b=>({...b,left:Array.from(b.left),right:Array.from(b.right)}))}})`);
  writeFileSync(join(output,'generation.json'), JSON.stringify(capture));
  writeFileSync(join(output,'trace-completion.json'),JSON.stringify(traceCompletion,null,2));
  // All acquisition is frozen and exported. Release the application before CPU
  // analysis; the oracle consumes only the retained receipts and installed data.
  provenance?.stop();
  await cdp.call('Page.navigate',{url:'about:blank'});
  const traceCoverage = requireCinematicTrace(trace,capture,traceCompletion);
  writeFileSync(join(output,'trace-coverage.json'),JSON.stringify(traceCoverage,null,2));
  const boundaries = sourceGenerationBoundaries(trace,capture.sources);
  assert(boundaries.some(b => b.attributed), 'no source received actual native render ownership');
  writeFileSync(join(output,'boundaries.json'),JSON.stringify(boundaries,null,2));
  const comparison = compareSourceGeneration(capture,boundaries);
  assert(boundaries.every(b => b.attributed || b.retiredFrame <= comparison.start),
    'unattributed source overlaps paired comparison interval');
  const pcm = Buffer.alloc(comparison.frames * 8);
  for (const block of capture.graph.blocks.filter(b => b.frame >= comparison.start)) {
    for (let i = 0; i < 128; i++) {
      const offset = (block.frame - comparison.start + i) * 8;
      pcm.writeFloatLE(block.left[i], offset);pcm.writeFloatLE(block.right[i], offset + 4);
    }
  }
  writeFileSync(join(output,'paired.f32le'),pcm);
  comparison.pcm = {format:'f32le',channels:2,bytes:pcm.length,
    sha256:createHash('sha256').update(pcm).digest('hex')};
  writeFileSync(join(output,'mix-comparison.json'),JSON.stringify(comparison,null,2));
  assert(capture.graph.returnedClockReads?.length, 'production movieTime return was not observed');
  requireMovieHorizon(capture.graph.returnedClockReads,comparison.rate);
  if (controlledCancellation) {
    const cancellation = requireCinematicCancellation(capture,boundaries);
    writeFileSync(join(output,'cancellation.json'),JSON.stringify(cancellation,null,2));
  }
  if(presentedWindow) {
    writeJsonArray(join(output,'trace.json'),trace);
    writeFileSync(join(output,'surfaces.json'),JSON.stringify(surfaces));
    await promisify(execFile)(process.env.CYRUS_ANALYSIS_PYTHON??'python3',[
      new URL('./cinematic-presented-frames.py',import.meta.url).pathname,output,presentedWindow,
      join(output,'presented.json'),'--minimum-frame',String(stopFrame),'--align']);
  }
  console.log(JSON.stringify({sources:boundaries.filter(b=>b.attributed).length,retiredUnattributed:boundaries.filter(b=>!b.attributed).length,surfaces:surfaces.length,movie:capture.graph.movie}));
} catch (error) {
  failure = String(error.stack ?? error);
  try {
    const partial=await evaluate(`({phase:globalThis.__cinematicSeekGate?.status(),seeks:globalThis.__cinematicSeeks,
      sources:globalThis.__sourceGeneration?.sources.map(s=>({...s,blocks:s.blocks.map(b=>({...b,...(b.zero?{}:{left:Array.from(b.left),right:Array.from(b.right)})}))})),
      graph:{blocks:globalThis.__sessionSamples?.blocks.slice(-200).map(b=>({...b,left:Array.from(b.left),right:Array.from(b.right)}))}})`);
    writeFileSync(join(output,'observer-failure.json'),JSON.stringify(partial));
  } catch(readbackError) {writeFileSync(join(output,'observer-failure-readback.txt'),String(readbackError));}
  throw error;
}
finally {
  try {if(recording) await cdp.call('Page.stopScreencast');if(tracing) await endTrace();}
  finally {
    writeJsonArray(join(output,'trace.json'),trace);
    writeFileSync(join(output,'surfaces.json'),JSON.stringify(surfaces,null,2));
    writeFileSync(join(output,'result.json'),JSON.stringify({failure},null,2));
    provenance?.stop();
    await cdp.close();
  }
}
