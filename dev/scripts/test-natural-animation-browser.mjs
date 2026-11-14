// Installed animation commands, actual shared sample ownership and rendered CPAL PCM.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint,page,wasm,output]=process.argv.slice(2);
assert(endpoint&&page&&wasm&&output,'usage: CDP PAGE WASM OUTPUT');
const expected=createHash('sha256').update(readFileSync(wasm)).digest('hex');
const cdp=await connectCdp(endpoint), pending=[],executed=[],errors=[];
async function evaluate(expression){const {result,exceptionDetails}=await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});assert(!exceptionDetails,result.description??exceptionDetails?.text);return result.value;}
try{
 cdp.onEvent(({method,params})=>{
  if(method==='Runtime.exceptionThrown'||(method==='Runtime.consoleAPICalled'&&params.type==='error'))errors.push(params);
  if(method==='Debugger.scriptParsed'&&params.scriptLanguage==='WebAssembly')pending.push(cdp.call('Debugger.getScriptSource',{scriptId:params.scriptId}).then(source=>{assert(source.bytecode);const hash=createHash('sha256').update(Buffer.from(source.bytecode,'base64')).digest('hex');assert.equal(hash,expected);executed.push(hash);}).catch(error=>errors.push(String(error))));
 });
 await cdp.call('Debugger.enable');await cdp.call('Page.enable');await cdp.call('Runtime.enable');
 await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:readFileSync(new URL('./session-sample-graph.js',import.meta.url),'utf8')+`
 globalThis.__naturalRows=[];
 const log=console.log;console.log=function(...args){for(const value of args){try{const row=JSON.parse(value);if(row.natural_animation)__naturalRows.push({...row,graph:__sessionSamples.clock()})}catch{}}log.apply(this,args)};
 `});
 await cdp.call('Page.navigate',{url:page});
 const deadline=performance.now()+40000;
 while(!await evaluate('__naturalRows.at(-1)?.kind==="complete"')){
  assert.equal(errors.length,0,JSON.stringify(errors));assert(performance.now()<deadline,'bounded natural animation runtime expired');
  writeFileSync(output+'.progress.json',JSON.stringify(await evaluate('__naturalRows'),null,2));await delay(100);
 }
 await delay(500);
 const rows=await evaluate('__naturalRows'),status=await evaluate('__sessionSamples.status()');
 assert(!status.error,status.error);assert(status.ready);
 const ownership=rows.filter(row=>row.kind==='ownership');const seen=new Set(),admissions=[];
 for(const row of ownership)for(const [slot,sample]of row.ownership.samples.slots.entries())if(sample&&!seen.has(sample.token)){seen.add(sample.token);admissions.push({...row,slot,sample});}
 assert.deepEqual(admissions.map(row=>row.sample.effect),[22,21,40]);assert.deepEqual(admissions.map(row=>row.slot),[0,0,0]);
 assert.deepEqual(admissions.map(row=>row.sample.owner.Script.index),[0,0,1]);
 assert.deepEqual(admissions.map(row=>row.sample.owner.Script.generation),[1,1,1]);
 assert.deepEqual(admissions.map(row=>row.phase),[1,3,3]);
 assert(!ownership.some(row=>row.phase===1&&row.frame>6&&row.ownership.samples.slots[0]===null),'draw completed before EndAnimation');
 assert(admissions.slice(0,2).every(row=>row.ownership.animation.length===1));assert.deepEqual(admissions[2].ownership.animation,[]);
 const ends=rows.filter(row=>row.kind==='end');assert.deepEqual(ends.map(row=>row.frame),[12,12]);assert(admissions[2].frame<ends[1].frame,'replacement admitted after EndAnimation');
 const plays=rows.filter(row=>row.kind==='command'&&row.sound.startsWith('Play'));assert.deepEqual(plays.map(row=>row.frame),[6,6]);
 const stop=rows.find(row=>row.kind==='replacement_survived');assert(stop);
 const windows=[];
 async function check(name,frame,startSeconds,endSeconds,present){
  const start=Math.ceil((frame+startSeconds*status.rate)/128)*128,end=Math.floor((frame+endSeconds*status.rate)/128)*128;
  const result=await evaluate(`(()=>{const graph=__sessionSamples;const metadata=graph.capture(${start},${end});const pcm=graph.capturedPcm;return {metadata,energy:pcm.reduce((sum,x)=>sum+x*x,0),samples:pcm.length,pcm:Array.from(pcm)}})()`);
  assert(present?result.energy>.001:result.energy<1e-12,`${name}: ${result.energy}`);
  const {pcm,...receipt}=result,bytes=Buffer.from(new Float32Array(pcm).buffer);
  writeFileSync(`${output}.${name}.f32`,bytes);
  windows.push({name,present,...receipt,pcmSha256:createHash('sha256').update(bytes).digest('hex')});
 }
 await check('draw',admissions[0].graph.now,.12,.3,true);
 await check('sheath',admissions[1].graph.now,.12,.3,true);
 await check('between-transitions',ends[0].graph.now,.5,1.0,false);
 await check('replacement-after-end',ends[1].graph.now,.5,1.0,true);
 await check('final-stop',stop.graph.now,.5,1.0,false);
 await Promise.all(pending);assert.equal(errors.length,0,JSON.stringify(errors));assert.deepEqual(executed,[expected]);
 writeFileSync(output,JSON.stringify({rows,windows,executedWasm:executed},null,2));
}finally{await cdp.close();}
