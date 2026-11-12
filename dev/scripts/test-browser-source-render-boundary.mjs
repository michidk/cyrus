// Attribute a command to native render processing, independently of PCM fitting.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {connectCdp} from './capture-cdp.mjs';

export function renderBoundaries(trace, trials) {
  const markers = trace.filter(e => e.name === 'TimeStamp');
  const handlers = trace.filter(e => e.name === 'AudioHandler::ProcessIfNecessary'
    && e.args?.['node type'] === 'AudioBufferSourceNode').sort((a, b) => a.ts - b.ts);
  const messages = e => e.args?.data?.message;
  return trials.map(trial => {
    const calls = markers.filter(e => messages(e) === `source:${trial.id}`);
    assert.equal(calls.length, 1, 'require one native command marker');
    const call = calls[0];
    const next = markers.filter(e => /^source:\d+$/.test(messages(e)) && e.ts > call.ts)
      .sort((a, b) => a.ts - b.ts)[0]?.ts ?? Infinity;
    const seen = new Set(handlers.filter(e => e.ts < call.ts).map(e => e.args.this));
    const fresh = new Map();
    for (const handler of handlers.filter(e => e.pid === call.pid && e.ts >= call.ts && e.ts < next)) {
      if (!seen.has(handler.args.this) && !fresh.has(handler.args.this)) fresh.set(handler.args.this, handler);
    }
    assert.equal(fresh.size, 1, 'require one isolated new source; concurrent sources are ambiguous');
    const handler = [...fresh.values()][0];
    assert(trace.some(e => e.name === 'AudioBufferSourceHandler::Process'
      && e.pid === handler.pid && e.tid === handler.tid
      && e.ts >= handler.ts && e.ts + e.dur <= handler.ts + handler.dur), 'native source did not process');
    const quantum = markers.filter(e => e.pid === handler.pid && e.tid === handler.tid
      && e.ts >= handler.ts && /^quantum:\d+$/.test(messages(e))).sort((a, b) => a.ts - b.ts)[0];
    assert(quantum && quantum.ts < next, 'native render has no generation-clock boundary');
    const firstRenderFrame = Number(messages(quantum).split(':')[1]);
    const onset = Math.max(firstRenderFrame, trial.requestedFrame);
    assert.equal(trial.first, onset, 'PCM onset disagrees with independently traced native rendering');
    assert.equal(trial.error, 0, 'generated waveform differs after traced admission');
    return {...trial, firstRenderFrame, onset, commandDelayFrames: onset - trial.requestedFrame};
  });
}

async function main() {
  const [endpoint, page, output] = process.argv.slice(2);
  assert(endpoint && page && output, 'usage: CDP_URL LOOPBACK_PAGE RESULT_JSON');
  const cdp = await connectCdp(endpoint), trace = [];
  let complete, tracing = false;
  const ended = new Promise(resolve => { complete = resolve; });
  async function finishTrace() {
    await cdp.call('Tracing.end', {}, null);
    let timer;
    try {
      await Promise.race([ended, new Promise((_, reject) => {
        timer = setTimeout(() => reject(Error('native render trace completion unavailable')), 30000);
      })]);
    } finally { clearTimeout(timer); }
    tracing = false;
  }
  cdp.onEvent(message => {
    if (message.method === 'Tracing.dataCollected') trace.push(...message.params.value);
    if (message.method === 'Tracing.tracingComplete') complete();
  });
  async function evaluate(expression) {
    const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {expression,
      returnByValue: true, awaitPromise: true, userGesture: true});
    assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
    return result.value;
  }
  try {
    await cdp.call('Page.enable');
    let ready = false;
    for (let i = 0; i < 100; i++) {
      if (!(await cdp.call('Page.navigate', {url: page})).errorText) { ready = true; break; }
      await new Promise(resolve => setTimeout(resolve, 25));
    }
    assert(ready, 'render boundary control page unavailable');
    await evaluate(`new Promise(resolve => {
      if (document.readyState === 'complete') resolve(); else addEventListener('load', resolve, {once:true});
    })`);
    await cdp.call('Tracing.start', {categories: 'disabled-by-default-webaudio.audionode,devtools.timeline', transferMode: 'ReportEvents'}, null);
    tracing = true;
    const graph = readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8')
      .replace('this.port.postMessage({frame,', 'console.timeStamp("quantum:" + frame);this.port.postMessage({frame,');
    const recording = await evaluate(`(async () => {
      const native = AudioBufferSourceNode.prototype.start;
      let id = 0;
      AudioBufferSourceNode.prototype.start = function(...args) {
        this.controlId = id++;
        console.timeStamp('source:' + this.controlId);
        const before = this.context.currentTime;
        const result = native.apply(this, args);
        this.nativeAdmission = {before, after: this.context.currentTime};
        return result;
      };
      ${graph}
      const context = new AudioContext({sampleRate:48000}), trials = [];
      const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
      try {
        await context.resume();
        const bootstrap = context.createBufferSource();
        bootstrap.buffer = context.createBuffer(2,128,context.sampleRate);
        bootstrap.connect(context.destination); bootstrap.start();
        for (let i=0;i<200&&!__sessionSamples.status().ready;i++) await wait(10);
        await wait(150);
        for (let i=0;i<24;i++) {
          const source = context.createBufferSource();
          source.buffer = context.createBuffer(2,1024,context.sampleRate);
          source.buffer.getChannelData(0).fill(.125);source.buffer.getChannelData(1).fill(.25);
          source.connect(context.destination);
          const requestedFrame = i%2 ? Math.ceil((context.currentTime+.05)*context.sampleRate/128)*128
            : Math.round(context.currentTime*context.sampleRate);
          const from = __sessionSamples.blocks.length;
          source.start(requestedFrame/context.sampleRate);
          // Exercise deferred graph adoption while the rendering thread runs.
          for (let j=0;j<600;j++) { const gain=context.createGain();gain.connect(context.destination);gain.disconnect(); }
          await wait(130);
          let first;
          for (const block of __sessionSamples.blocks.slice(from)) {
            for(let at=0;at<128;at++) if(block.left[at]!==0) {first=block.frame+at;break;}
            if(first!==undefined)break;
          }
          let error=0;
          for(const block of __sessionSamples.blocks.slice(from)) for(let at=0;at<128;at++) {
            const active=block.frame+at>=first && block.frame+at<first+1024;
            error+=(block.left[at]-(active ? 0.125 : 0))**2+(block.right[at]-(active ? 0.25 : 0))**2;
          }
          trials.push({id:source.controlId,requestedFrame,first,error,native:source.nativeAdmission});
        }
        const p=__sessionSamples;
        const metadata=p.capture(p.blocks[0].frame,p.blocks.at(-1).frame+128);
        return {trials,rate:context.sampleRate,graphError:metadata.error,quanta:metadata.quanta};
      } finally { await context.close(); }
    })()`);
    await finishTrace();
    writeFileSync(output + '.trace.json', JSON.stringify(trace));
    // Preserve evidence before attribution can reject it.
    writeFileSync(output, JSON.stringify(recording, null, 2) + '\n');
    assert.equal(recording.graphError, null);
    recording.boundaries = renderBoundaries(trace, recording.trials);
    writeFileSync(output, JSON.stringify(recording, null, 2) + '\n');
    console.log(JSON.stringify({trials: recording.trials.length,
      delayed: recording.boundaries.filter(b => b.commandDelayFrames > 0).map(b => ({id:b.id,frames:b.commandDelayFrames}))}));
  } catch (error) {
    writeFileSync(output + '.failure.json', JSON.stringify({error:String(error)}, null, 2) + '\n');
    throw error;
  } finally {
    try {
      if (tracing) await finishTrace();
    } finally {
      writeFileSync(output + '.trace.json', JSON.stringify(trace));
      await cdp.close();
    }
  }
}
if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
