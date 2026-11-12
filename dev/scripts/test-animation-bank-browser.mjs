// Installed animation-emitter admission, remembered stops and world-owner replacement.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
import {observeBrowserBuild} from './browser-provenance.mjs';
const [endpoint,page,install,output,effect='40',scene='ISLAND',emitter='DR_SPIN6']=process.argv.slice(2);
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
  let actor;
  for(let offset=0;offset<2048;offset+=64){
    await evaluate(`window.__cyrusInspectScriptsOffset=${offset};true`);
    const rows=await until('installed emitter page',`__cyrusCapture.scripts?.actors?.[0]?.id.index===${offset}&&__cyrusCapture.scripts.actors`);
    actor=rows.find(row=>row.name===emitter);if(actor)break;if(rows.length<64)break;
  }
  assert(actor,'installed animated emitter absent');
  const home=actor.spatial.effective?.position??actor.spatial.locations[0];assert(home?.length===3);
  async function action(fields){
    const current=++id;
    await evaluate(`window.__cyrusAcceptanceRequest=${JSON.stringify(JSON.stringify({id:current,...fields}))};true`);
    const response=await until('public scene action',`__cyrusAcceptanceResult?.id===${current}&&__cyrusAcceptanceResult`);
    assert(response.applied,response.error);
  }
  const point=home.map(v=>Math.floor(v/256));
  await action({action:'place_player',x:point[0],y:point[1],z:point[2],heading:0});
  await action({action:'reset_camera'});
  await until('camera reset consumed','__cyrusCapture.scripts.globals.camera_reset_request===false',600);
  await until('emitter family admitted',`__cyrusCapture.scripts.actors.find(row=>row.name===${JSON.stringify(emitter)})?.spatial.effective`);
  await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',key:'F12',code:'F12',windowsVirtualKeyCode:123});
  await delay(100);await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',key:'F12',code:'F12',windowsVirtualKeyCode:123});
  await until('console inhibition','__cyrusStartup.console_open===true');
  const index=actor.id.index;
  async function animation(type,generation,fields={}){return sample('animation',{generation,index,command:{type,...fields}});}
  let generation=await evaluate('__cyrusCapture.scripts.generation');
  await sample('stop');await record('initial-silence',0,false);
  assert(!(await session('save',{key:'animation-owner'})).error);
  await animation('play',generation,{effect:Number(effect)});await record('installed-animation',1);
  await animation('stop',generation);await record('animation-stop',0,false);
  await evaluate(`(()=>{const read=cyrusGameAssets.read.bind(cyrusGameAssets);window.__bankControl={hold:false,waiting:false};cyrusGameAssets.read=async path=>{if(path==='sounds/bank.sounds'&&__bankControl.hold){__bankControl.waiting=true;await new Promise(resolve=>__bankControl.release=resolve)}return read(path)};return true})()`);
  async function hold(){await evaluate('__bankControl.hold=true;__bankControl.waiting=false;true');await sample('reload_bank');await until('actual pending bank read','__bankControl.waiting');assert((await sample('inspect')).bank_pending);}
  async function release(){await evaluate('__bankControl.hold=false;__bankControl.release();true');const end=performance.now()+15000;while((await sample('inspect')).bank_pending){assert(performance.now()<end,'bank admission deadline');await delay(100);}}
  await hold();await animation('play',generation,{effect:Number(effect)});
  await record('pending-no-premature-output',0,false);
  await animation('stop',generation);await release();await record('cancelled-before-admission',0,false);
  await hold();
  for(let i=0;i<2;i++)await animation('play',generation,{effect:Number(effect)});
  await animation('stop',generation);await release();
  await record('only-forgotten-older-start',1);
  await animation('stop',generation);await record('older-start-not-owned-by-end-animation',1);
  await sample('stop');await record('global-stop',0,false);
  await hold();await animation('play',generation,{effect:Number(effect)});
  const loaded=await session('load',{key:'animation-owner'});assert(!loaded.error,loaded.error);
  const fresh=await until('fresh world owner',`__cyrusCapture.scripts?.generation!==${generation}&&__cyrusCapture.scripts.generation`);
  await animation('play',fresh,{effect:Number(effect)});
  await record('replacement-still-pending',0,false);
  await release();await record('only-fresh-owner-admitted',1);
  await record('fresh-admitted-once',1);
  await animation('stop',fresh);await record('fresh-stop',0,false);
  writeFileSync(`${output}/result.json`,JSON.stringify({stages,provenance:await provenance.verify()},null,2));
}catch(error){writeFileSync(`${output}/failure.json`,JSON.stringify({error:String(error),stages,observation:await evaluate('({startup:window.__cyrusStartup,capture:window.__cyrusCapture,inspection:window.__cyrusSampleInspection})').catch(()=>null)},null,2));throw error;}
finally{provenance?.stop();await cdp.close();}
