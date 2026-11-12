// Non-preview isolated public SOUP callers with actual production CPAL graph output.
import assert from 'node:assert/strict';
import {checkPcm,checkLevels,expected} from './test-positional-pcm.mjs';
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint, page, wasmFile, output, count="13"] = process.argv.slice(2);
assert(endpoint && page && wasmFile && output, 'usage: node dev/scripts/test-positional-browser.mjs CDP_URL PAGE_URL WASM_FILE OUTPUT_DIR');
const stages=Number(count);
assert(Number.isInteger(stages)&&stages>0&&stages<=13,"invalid stage count");
mkdirSync(output,{recursive:true});
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const expectedWasm = digest(readFileSync(wasmFile));
const observedWasm = [], pending = [], failures = [];

const checkpoint = `${output}/result.json`;
const saved = existsSync(checkpoint) ? JSON.parse(readFileSync(checkpoint, 'utf8')) : null;
if (saved) {
  assert.deepEqual(saved.executedWasm, [expectedWasm], 'checkpoint WASM provenance differs');
  assert(saved.results.length <= stages, 'invalid checkpoint length');
  for (const [phase, stage] of saved.results.entries()) {
    assert.equal(stage.phase, phase);
    assert(stage.pcm.frames >= 8192, 'checkpoint lacks complete PCM observation');
    assert.deepEqual(stage.pcm, checkLevels(stage,stage.pcm.rms,stage.pcm.frames));
  }
}
const results = saved?.results ?? [], runtimeErrors = [];
let phase = 0;
const cdp = await connectCdp(endpoint);
async function evaluate(expression) {
  const {result,exceptionDetails} = await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
  return result.value;
}
try {
  cdp.onEvent(({method,params}) => {
    if(method==='Runtime.exceptionThrown' || (method==='Runtime.consoleAPICalled' && params.type==='error'))runtimeErrors.push(params);
    if(method === 'Debugger.scriptParsed' && params.scriptLanguage === 'WebAssembly') {
      pending.push(cdp.call('Debugger.getScriptSource',{scriptId:params.scriptId}).then(source => {
        assert(source.bytecode,'executed WASM bytes absent');
        const hash=digest(Buffer.from(source.bytecode,'base64'));
        assert.equal(hash,expectedWasm,'executed WASM differs from supplied build');
        observedWasm.push(hash);
      }).catch(error=>failures.push(error)));
    }
  });
  await cdp.call('Debugger.enable');
  await cdp.call('Page.enable');
  await cdp.call('Runtime.enable');
  await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:
    readFileSync(new URL('./session-sample-graph.js',import.meta.url),'utf8')+'\n'+
    readFileSync(new URL('./browser-sound-reset.js',import.meta.url),'utf8')+`
    window.__callerStages=[];
    const log=console.log;
    console.log=function(...args){for(const value of args){try{const stage=JSON.parse(value);if(stage.positional_emitters)__callerStages.push(stage)}catch{}}log.apply(this,args)};
  `});
  await cdp.call('Page.navigate',{url:page});
  const deadline = performance.now()+85000;
  while (phase < stages) {
    assert.equal(failures.length,0,failures.map(String).join('\n'));
    assert.equal(runtimeErrors.length,0,JSON.stringify(runtimeErrors));
    assert(performance.now()<deadline, 'bounded SOUP output acceptance expired');
    const stage = await evaluate(`window.__callerStages?.[${phase}]`);
    if (!stage) {await delay(50);continue;}
    assert.equal(stage.phase,phase);
    if (phase < results.length) {
      const {pcm, ...previous} = results[phase];
      assert.deepEqual(stage, previous, 'replayed fixture differs from checkpoint');
      phase++;
      continue;
    }
    await delay(150); // SoundPlugin consumes this update's emitted cues before observation.
    while (!await evaluate('__sessionSamples.status().ready')) {
      assert.equal(failures.length,0,failures.map(String).join('\n'));
      assert.equal(runtimeErrors.length,0,JSON.stringify(runtimeErrors));
      assert(performance.now()<deadline,'sample graph never became ready');
      const error=await evaluate('__sessionSamples.status().error');
      assert(!error,error);
      await delay(50);
    }
    const window = await evaluate(`__cyrusSoundReset.sample(${JSON.stringify(`stage-${results.length}`)},${expected(stage).some(x=>x>0)})`);
    const samples = await evaluate(`__sessionSamples.blocks.filter(b=>b.frame>=${window.start}&&b.frame<${window.end}).map(b=>({left:Array.from(b.left),right:Array.from(b.right)}))`);
    writeFileSync(`${output}/stage-${phase}.json`,JSON.stringify({stage,window,samples}));
    const pcm=checkPcm(stage,samples.flatMap(b=>b.left),samples.flatMap(b=>b.right));
    results.push({...stage,pcm});
    phase++;
    await Promise.all(pending);
    assert.deepEqual(observedWasm,[expectedWasm]);
    writeFileSync(`${output}/result.json`,JSON.stringify({results,executedWasm:observedWasm},null,2));
  }
  await Promise.all(pending);
  assert.equal(failures.length,0,failures.map(String).join('\n'));
  assert.deepEqual(observedWasm,[expectedWasm]);
  console.log(JSON.stringify({results,executedWasm:observedWasm},null,2));
} catch(error) {
  const diagnostic=await evaluate('({status:__sessionSamples?.status(),frames:__sessionSamples?.blocks.map(b=>b.frame),stages:window.__callerStages})').catch(e=>String(e));
  writeFileSync(`${output}/failure.json`,JSON.stringify({phase:results.length,diagnostic,events:cdp.events},null,2));
  throw error;
}
finally {await cdp.close();}
