// Compare every captured sample with the engine's independently indexed offline output.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint, page] = process.argv.slice(2);
assert(endpoint && page, 'usage: test-browser-sample-epochs.mjs CDP_URL LOOPBACK_PAGE');
const cdp = await connectCdp(endpoint);
const graph = readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8');
try {
  await cdp.call('Page.enable');
  for (let i = 0; i < 40; i++) {
    if (!(await cdp.call('Page.navigate', {url: page})).errorText) break;
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  let ready = false;
  for (let i = 0; i < 200; i++) {
    const state = await cdp.call('Runtime.evaluate', {expression: `location.href === ${JSON.stringify(page)} && document.readyState === 'complete'`, returnByValue: true});
    if (state.result.value) { ready = true; break; }
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  assert(ready, 'offline clock control page deadline');
  const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {awaitPromise: true, returnByValue: true,
    expression: `(async () => {
      ${graph}
      const rate = 44100, length = Math.ceil(132 * rate / 128) * 128;
      const context = new OfflineAudioContext(2, length, rate);
      const source = context.createBufferSource();
      source.buffer = context.createBuffer(2, 4096, rate);
      for(let c=0;c<2;c++)for(let i=0;i<4096;i++)source.buffer.getChannelData(c)[i]=Math.sin(i*.1)*.125;
      source.connect(context.destination);source.start(130);
      const p=__sessionSamples;
      for(let i=0;i<200&&!p.status().ready;i++){if(p.error)throw Error(p.error);await new Promise(r=>setTimeout(r,10));}
      if(!p.status().ready)throw Error('tap readiness deadline');
      const rendered=await context.startRendering();
      for(let i=0;i<200&&p.blocks.at(-1)?.frame<length-128;i++)await new Promise(r=>setTimeout(r,10));
      let metadata;try{metadata=p.capture(p.startFrame,length);}catch(error){throw Error(JSON.stringify({error:String(error),armed:p.startFrame,first:p.blocks[0],failed:p.blocks.find(b=>b.error)}));}
      let error=0,energy=0;
      for(let i=0;i<p.capturedPcm.length;i++){const actual=p.capturedPcm[i],expected=rendered.getChannelData(i%2)[p.startFrame+Math.floor(i/2)];error+=(actual-expected)**2;energy+=actual*actual;}
      if(!(energy>.001&&error===0))throw Error(JSON.stringify({energy,error}));
      return {quanta:metadata.quanta.length,start:metadata.start,end:metadata.end,rate,energy,error};
    })()`});
  assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
  console.log(JSON.stringify(result.value));
} finally { await cdp.close(); }
