// Non-preview isolated public SOUP callers with actual production CPAL graph output.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint, page, wasmFile] = process.argv.slice(2);
assert(endpoint && page && wasmFile, 'usage: node dev/scripts/test-sound-callers-browser.mjs CDP_URL PAGE_URL WASM_FILE');
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const expectedWasm = digest(readFileSync(wasmFile));
const observedWasm = [], pending = [], failures = [];

const cdp = await connectCdp(endpoint);
const results = [], runtimeErrors = [];
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
    console.log=function(...args){for(const value of args){try{const stage=JSON.parse(value);if(stage.sound_callers)__callerStages.push(stage)}catch{}}log.apply(this,args)};
  `});
  await cdp.call('Page.navigate',{url:page});
  const deadline = performance.now()+85000;
  while (results.length < 18) {
    assert.equal(failures.length,0,failures.map(String).join('\n'));
    assert.equal(runtimeErrors.length,0,JSON.stringify(runtimeErrors));
    assert(performance.now()<deadline, 'bounded SOUP output acceptance expired');
    const stage = await evaluate(`window.__callerStages?.[${results.length}]`);
    if (!stage) {await delay(50);continue;}
    assert.equal(stage.phase,results.length%9);
    assert.equal(stage.emitted,[4,1,1,2,0,1,1,1,0][stage.phase]);
    assert.equal(stage.shells,results.length<9);
    assert.equal(stage.numeric_rejected,stage.phase>=7);
    if(stage.phase>=5)assert.deepEqual(stage.flags,[7]);
    await delay(150); // SoundPlugin consumes this update's emitted cues before observation.
    while (!await evaluate('__sessionSamples.status().ready')) {
      assert.equal(failures.length,0,failures.map(String).join('\n'));
    assert.equal(runtimeErrors.length,0,JSON.stringify(runtimeErrors));
      assert(performance.now()<deadline,'sample graph never became ready');
      const error=await evaluate('__sessionSamples.status().error');
      assert(!error,error);
      await delay(50);
    }
    const pcm = await evaluate(`__cyrusSoundReset.sample(${JSON.stringify(`stage-${results.length}`)},${stage.expected_channels>0})`);
    results.push({...stage,pcm});
  }
  await Promise.all(pending);
  assert.equal(failures.length,0,failures.map(String).join('\n'));
  assert.deepEqual(observedWasm,[expectedWasm]);
  console.log(JSON.stringify({results,executedWasm:observedWasm},null,2));
} catch(error) { console.error(JSON.stringify({events:cdp.events},null,2)); throw error; }
finally {await cdp.close();}
