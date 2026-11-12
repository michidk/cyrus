// Public deferred cue, checkpoint rejection and replacement with actual graph output.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint,page,wasm,output]=process.argv.slice(2);
assert(endpoint&&page&&wasm&&output,'usage: CDP_URL PAGE_WITH_EXPLICIT_FIXTURE WASM OUTPUT_JSON');
assert(new URL(page).searchParams.has('fixture'),'explicit installed-source fixture required');
const expected=createHash('sha256').update(readFileSync(wasm)).digest('hex');
const cdp=await connectCdp(endpoint), results=[], observed=[], checks=[], errors=[];
async function evaluate(expression){
  const {result,exceptionDetails}=await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  assert(!exceptionDetails,result.description??exceptionDetails?.text);return result.value;
}
async function until(label,expression){
  const deadline=performance.now()+30000;
  while(performance.now()<deadline){const value=await evaluate(expression);if(value)return value;await delay(50);}
  throw Error(`bounded deferred capture expired: ${label}`);
}
async function action(name,playing,error,present){
  const index=results.length;
  await evaluate(`window.__deferredRequest=${JSON.stringify(name)};true`);
  const state=await until(name,`window.__deferredStages?.[${index}]`);
  assert.equal(state.action,name);assert.equal(state.playing,playing,`${name}: expected ${playing} audible transports, got ${state.playing}`);
  if(error)assert.match(state.error,error);else assert.equal(state.error,null);
  await until('graph ready','window.__sessionSamples?.status().ready');
  const pcm=await evaluate(`__cyrusSoundReset.sample(${JSON.stringify(`${index}-${name}`)},${present})`);
  results.push({...state,pcm});writeFileSync(output,JSON.stringify({expected,observed,results},null,2));
}
try{
  cdp.onEvent(({method,params})=>{
    if(method==='Debugger.scriptParsed'&&params.scriptLanguage==='WebAssembly'){
      checks.push(cdp.call('Debugger.getScriptSource',{scriptId:params.scriptId}).then(source=>{
        assert(source.bytecode,'executed bytecode absent');
        const hash=createHash('sha256').update(Buffer.from(source.bytecode,'base64')).digest('hex');
        assert.equal(hash,expected);observed.push(hash);
      }).catch(error=>errors.push(String(error))));
    }
  });
  await cdp.call('Debugger.enable');await cdp.call('Page.enable');
  await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:
    readFileSync(new URL('./session-sample-graph.js',import.meta.url),'utf8')+'\n'+
    readFileSync(new URL('./browser-sound-reset.js',import.meta.url),'utf8')+`
    window.__deferredStages=[];
    const original=console.log;
    console.log=function(...args){for(const arg of args){try{const state=JSON.parse(arg);if(state.deferred_capture)__deferredStages.push(state)}catch{}}original.apply(this,args)};
  `});
  await cdp.call('Page.navigate',{url:page});
  await until('fixture ready','typeof window.captureAction==="function"');
  await action('start',1,null,true);
  await action('defer',1,null,true);
  await action('capture',1,/deferred sample starts/,true);
  await action('admit',2,null,true);
  await action('capture',2,null,true);
  await action('reject',2,/installed source absent/,true);
  await action('restore',2,null,true);
  await action('stop',0,null,false);
  await action('defer',0,null,false);
  await action('stop',0,null,false);
  await action('admit',0,null,false);
  await action('start',1,null,true);
  await action('stop',0,null,false);
  await Promise.all(checks);assert.deepEqual(errors,[]);assert.deepEqual(observed,[expected]);
  writeFileSync(output,JSON.stringify({expected,observed,results},null,2));
}catch(error){writeFileSync(`${output}.failure.json`,JSON.stringify({error:String(error),results,events:cdp.events},null,2));throw error;}
finally{await cdp.close();}
