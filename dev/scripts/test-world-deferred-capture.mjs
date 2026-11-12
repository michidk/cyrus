// Full installed-world session admission with a real deferred browser bank read.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
import {observeBrowserBuild} from './browser-provenance.mjs';
const [endpoint,page,install,output,effect='40',scene='ISLAND']=process.argv.slice(2);
assert(endpoint&&page&&install&&output,'usage: CDP PAGE INSTALL OUTPUT [LOOP_EFFECT] [SCENE]');
mkdirSync(output,{recursive:true});
const url=new URL(page);url.search=new URLSearchParams({capture:'1',scene,install:new URL(install).href});
const cdp=await connectCdp(endpoint), stages={};let id=0,provenance;
async function evaluate(expression){
  const {result,exceptionDetails}=await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  assert(!exceptionDetails,result.description??exceptionDetails?.text);return result.value;
}
async function until(label,expression,seconds=90){
  const end=performance.now()+seconds*1000;
  while(performance.now()<end){const result=await evaluate(expression);if(result)return result;await delay(100);}
  throw Error(`bounded world capture expired: ${label}`);
}
async function session(type,fields={}){
  async function send(type,fields){
    const current=++id;
    await evaluate(`window.__cyrusSessionRequest=${JSON.stringify(JSON.stringify({type,id:current,...fields}))};true`);
    return until(type,`window.__cyrusSessionResult?.id===${current}&&window.__cyrusSessionResult`);
  }
  let result=await send(type,fields);const end=performance.now()+90000;
  while(result.pending){assert(performance.now()<end,'session completion deadline');await delay(100);result=await send('result',{});}
  return result;
}
async function sample(action,fields={}){
  const current=++id;
  await evaluate(`window.__cyrusSampleRequest=${JSON.stringify(JSON.stringify({action,id:current,...fields}))};true`);
  return until(action,`window.__cyrusSampleInspection?.id===${current}&&window.__cyrusSampleInspection`);
}
async function playing(count){
  const end=performance.now()+15000;
  for(;;){const state=await sample('inspect');if(state.playing===count)return state;assert(performance.now()<end,`expected ${count} transports, got ${state.playing}`);await delay(100);}
}
async function record(name,count,present=true){
  const state=await playing(count);
  const pcm=await evaluate(`__cyrusSoundReset.sample(${JSON.stringify(name)},${present})`);
  stages[name]={state,pcm};writeFileSync(`${output}/progress.json`,JSON.stringify(stages,null,2));return state;
}
async function stored(database='cyrus-sessions',key='world-deferred'){
  return evaluate(`(async()=>{const db=await new Promise((resolve,reject)=>{const r=indexedDB.open(${JSON.stringify(database)},1);r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error)});try{return await new Promise((resolve,reject)=>{const r=db.transaction('documents').objectStore('documents').get(${JSON.stringify(key)});r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(r.error)})}finally{db.close()}})()`);
}
function body(result){assert(!result.error,result.error);return JSON.parse(result.document.split('\n').slice(2).join('\n'));}
try{
  provenance=await observeBrowserBuild(cdp,JSON.parse(readFileSync('target/web/build-provenance.json')),url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});
  await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:readFileSync(new URL('./session-sample-graph.js',import.meta.url),'utf8')+'\n'+readFileSync(new URL('./browser-sound-reset.js',import.meta.url),'utf8')+'\nwindow.__cyrusInspectScripts=true;'});
  await cdp.call('Page.navigate',{url:url.href});
  await until('explicit install','document.getElementById("install-picker")');
  assert.equal(await evaluate('document.getElementById("install-url").value'),new URL(install).href);
  await evaluate('document.getElementById("install-picker").requestSubmit();true');
  await until('prepared world','window.__cyrusCapture?.ready&&__cyrusCapture.scripts?.globals?.world_loop_frame>10',600);
  // Require a full successful public capture, not merely advancing global time.
  let initial;const end=performance.now()+90000;
  do{initial=await session('capture');if(!initial.error)break;assert(performance.now()<end,initial.error);await delay(250);}while(true);
  const before=body(initial);writeFileSync(`${output}/initial.json`,JSON.stringify(before,null,2));
  await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',key:'F12',code:'F12',windowsVirtualKeyCode:123});
  await delay(100);await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',key:'F12',code:'F12',windowsVirtualKeyCode:123});
  await until('console inhibition','__cyrusStartup.console_open===true');
  await sample('stop');await record('silence',0,false);
  const actors=await evaluate('__cyrusCapture.scripts');
  const owner={generation:actors.generation,index:actors.actors[0].id.index,effect:Number(effect)};
  await sample('script_play',owner);await record('live-loop',1);
  assert(!(await session('save',{key:'world-deferred'})).error);
  const oldDocument=await stored();assert.equal(typeof oldDocument,'string');
  const accepted=await session('slots',{operation:{type:'create',description:'ACCEPTED'}});assert(!accepted.error,accepted.error);
  const selected=(await sample('inspect')).selected;assert(selected);
  const slotsBefore=await session('slots',{operation:{type:'list'}});
  const oldSlot=await stored('cyrus-save-slots',['payload',selected.slot]);
  await evaluate(`(()=>{const read=cyrusGameAssets.read.bind(cyrusGameAssets);window.__bankRead={pending:false,released:false};cyrusGameAssets.read=async path=>{if(path==='sounds/bank.sounds'&&!__bankRead.released){__bankRead.pending=true;await new Promise(resolve=>window.__releaseBank=()=>{__bankRead.released=true;resolve()});}return read(path)};return true})()`);
  await sample('reload_bank');await until('real reader pending','__bankRead.pending');
  const bank=await sample('inspect');assert(bank.bank_pending,'real reload admission gate prematurely retired');
  await sample('script_play',owner);await record('pending-loop',1);
  for(const [name,type,fields] of [['named','save',{key:'world-deferred'}],['slot','slots',{operation:{type:'create',description:'REJECTED'}}]]){
    const result=await session(type,fields);assert.match(result.error,/deferred sample starts/);
    assert.equal(await stored(),oldDocument,'rejected save replaced valid document');
    assert.equal(await stored('cyrus-save-slots',['payload',selected.slot]),oldSlot,'rejected capture replaced valid slot payload');
    const slotsAfter=await session('slots',{operation:{type:'list'}});assert.deepEqual(slotsAfter.slots,slotsBefore.slots,'rejected capture published slot');
    const state=await record(`rejected-${name}`,1);assert.deepEqual(state.selected,selected,'rejected capture changed retained valid save');
  }
  await evaluate('__releaseBank();true');
  await until('reader released','__bankRead.released');
  await record('admitted-once',2);
  const saved=await session('save',{key:'world-deferred'});assert(!saved.error,saved.error);
  const document=await stored();assert.notEqual(document,oldDocument);
  const admittedSlot=await session('slots',{operation:{type:'create',description:'ADMITTED'}});assert(!admittedSlot.error,admittedSlot.error);
  assert.equal((await sample('inspect')).selected.description,'ADMITTED');
  const snapshot=body({document});writeFileSync(`${output}/saved.json`,JSON.stringify(snapshot,null,2));
  await record('still-exactly-two',2);
  await sample('stop');await record('before-restore',0,false);
  const loaded=await session('load',{key:'world-deferred'});assert(!loaded.error,loaded.error);
  await record('restored',2);
  const restored=body(await session('capture'));writeFileSync(`${output}/restored.json`,JSON.stringify(restored,null,2));
  const freshGeneration=restored.logical.registry.worlds.find(w=>w.world===restored.logical.registry.current).generation;
  assert.notEqual(freshGeneration,actors.generation,'restore must bind fresh world owners');
  assert.equal(snapshot.samples.players.length,2);assert.equal(restored.samples.players.length,2);
  const savedSlots=snapshot.samples.ownership.samples.slots.filter(Boolean);
  const freshSlots=restored.samples.ownership.samples.slots.filter(Boolean);
  assert.equal(savedSlots.length,2);assert.equal(freshSlots.length,2);
  for(const slot of freshSlots){assert.deepEqual(slot.owner,{Script:{generation:freshGeneration,index:owner.index}});assert.equal(slot.effect,Number(effect));assert(slot.token>snapshot.samples.ownership.samples.next_token);}
  for(const [index,player] of restored.samples.players.entries()){
    const saved=snapshot.samples.players[index];
    for(const field of ['source_identity','width','gains','repetitions','speed'])assert.deepEqual(player.transport[field],saved.transport[field],`restored decoder ${field}`);
    assert(player.transport.input_samples>0,'restored decoder did not advance');
  }
  await sample('stop');await record('final-stop',0,false);
  writeFileSync(`${output}/result.json`,JSON.stringify({stages,provenance:await provenance.verify()},null,2));
}catch(error){writeFileSync(`${output}/failure.json`,JSON.stringify({error:String(error),stages,observation:await evaluate('({startup:window.__cyrusStartup,capture:window.__cyrusCapture,inspection:window.__cyrusSampleInspection})').catch(()=>null)},null,2));throw error;}
finally{provenance?.stop();await cdp.close();}
