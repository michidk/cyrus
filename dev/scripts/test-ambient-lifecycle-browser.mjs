// Authored room ambient output and shared-pool teardown through public scene admission.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
import {observeBrowserBuild} from './browser-provenance.mjs';
const [endpoint,page,install,output,emitter,effect,scene='ISLAND',destination='START'] = process.argv.slice(2);
assert(endpoint&&page&&install&&output&&emitter&&effect,'usage: CDP_URL PAGE_URL INSTALL_URL OUTPUT EMITTER LOOP_EFFECT [SCENE] [DESTINATION]');
mkdirSync(output,{recursive:true});
const url=new URL(page);url.search=new URLSearchParams({capture:'1',scene,install:new URL(install).href});
const cdp=await connectCdp(endpoint);let provenance,id=0;
const results={stages:{}};
async function evaluate(expression){
  const {result,exceptionDetails}=await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  assert(!exceptionDetails,result.description??exceptionDetails?.text);return result.value;
}
async function until(label,expression,seconds=90){
  const deadline=performance.now()+seconds*1000;
  while(performance.now()<deadline){const value=await evaluate(expression);if(value)return value;await delay(100);}
  throw Error(`bounded operation expired: ${label}`);
}
async function ready(name,previous){
  return until(`room ${name}`,`window.__cyrusCapture?.ready && __cyrusCapture.world===${JSON.stringify(name)} && __cyrusCapture.scripts && ${previous===undefined?'true':`__cyrusCapture.scripts.generation!==${previous}`} && __cyrusStartup.state==='InGame'`,600);
}
async function progress(){
  const initial=await evaluate('__cyrusStartup.clock.game_seconds');
  await until('game-clock continuation',`__cyrusStartup.clock.game_seconds>=${initial+.4}`);
}
async function consoleOpen(open){
  if(await evaluate('!!__cyrusStartup.console_open')===open)return;
  await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',key:'F12',code:'F12',windowsVirtualKeyCode:123});
  await delay(100);
  await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',key:'F12',code:'F12',windowsVirtualKeyCode:123});
  await until('console boundary',`__cyrusStartup.console_open===${open}`);
}
async function action(request){
  const deadline=performance.now()+90000;
  for(;;){
    const current=++id;
    await evaluate(`window.__cyrusAcceptanceRequest=${JSON.stringify(JSON.stringify({...request,id:current}))};true`);
    const response=await until('public action',`window.__cyrusAcceptanceResult?.id===${current}&&window.__cyrusAcceptanceResult`);
    if(response.applied)return;
    // Scene admission precedes deferred player spatial preparation on slow hosts.
    // A rejected placement has no effect; retry only this explicit readiness error.
    assert(request.action==='place_player' && /spatial state unavailable/.test(response.error)
      && performance.now()<deadline,response.error);
    await consoleOpen(false);await progress();
  }
}
async function resetCamera(){
  await action({action:'reset_camera'});
  await consoleOpen(false);
  // A rendered scene and live global clock can precede asynchronous script
  // preparation. Require the ordinary camera owner to consume this request.
  await until('ordinary camera reset consumption',
    '__cyrusCapture.scripts.globals.camera_reset_request===false',600);
}
async function sample(action){
  const current=++id;
  await evaluate(`window.__cyrusSampleRequest=${JSON.stringify(JSON.stringify({action,id:current,...(action==='play'?{effect:Number(effect)}:{})}))};true`);
  await until('shared sample request',`window.__cyrusSampleResult===${current}`);
}
async function record(name,present){
  await until('graph ready','window.__sessionSamples?.status().ready');
  const pcm=await evaluate(`__cyrusSoundReset.sample(${JSON.stringify(name)},${present})`);
  const state=await evaluate('({startup:__cyrusStartup,world:__cyrusCapture.world,scripts:__cyrusCapture.scripts})');
  results.stages[name]={pcm,state};
  results.provenance=await provenance.verify();
  writeFileSync(`${output}/progress.json`,JSON.stringify(results,null,2));
}
try{
  provenance=await observeBrowserBuild(cdp,JSON.parse(readFileSync('target/web/build-provenance.json')),url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});
  await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:readFileSync(new URL('./session-sample-graph.js',import.meta.url),'utf8')+'\n'+readFileSync(new URL('./browser-sound-reset.js',import.meta.url),'utf8')+'\nwindow.__cyrusInspectScripts=true;'});
  await cdp.call('Page.navigate',{url:url.href});
  await until('explicit install picker', 'document.getElementById("install-picker") !== null');
  assert.equal(await evaluate('document.getElementById("install-url").value'), new URL(install).href,
    'the public picker must offer the explicitly supplied install');
  await evaluate('document.getElementById("install-picker").requestSubmit();true');
  await ready(scene);await progress();
  let actor;
  for(let offset=0;offset<2048;offset+=64){
    await evaluate(`window.__cyrusInspectScriptsOffset=${offset};true`);
    const rows=await until('actor observation',`__cyrusCapture.scripts?.actors?.[0]?.id.index===${offset}&&__cyrusCapture.scripts.actors`);
    actor=rows.find(row=>row.name===emitter);if(actor)break;
    if(rows.length<64)break;
  }
  assert(actor,'selected installed emitter absent');
  writeFileSync(`${output}/emitter.json`,JSON.stringify(actor,null,2));
  // Attached actors acquire an effective pose during ordinary family admission.
  // Their authored home is public placement data, unlike attachment-local offsets.
  const home=actor.spatial.effective?.position??actor.spatial.locations[0];
  assert(home?.length===3,'selected emitter has no public home placement');
  let point=home.map(v=>Math.floor(v/256));
  await action({action:'place_player',x:point[0],y:point[1],z:point[2],heading:0});
  await resetCamera();
  await consoleOpen(false);await progress();
  const admitted=await until('attached emitter family admission',
    `__cyrusCapture.scripts.actors.find(row=>row.name===${JSON.stringify(emitter)})?.spatial.effective`);
  point=admitted.position.map(v=>Math.floor(v/256));
  for(const [name,dx] of [['room-near',0],['room-moved',512]]){
    await action({action:'place_player',x:point[0]+dx,y:point[1],z:point[2],heading:0});
    await resetCamera();
    await consoleOpen(false);await progress();await consoleOpen(true);await record(name,true);
  }
  await sample('stop');await record('stopped',false);
  await sample('play');await record('unowned-start',true);
  const old=await evaluate('__cyrusCapture.scripts.generation');
  await action({action:'load_scene',name:destination});await ready(destination,old);
  await record('retired',false);
  await sample('play');await record('replacement-start',true);
  await sample('stop');await record('replacement-stop',false);
  const replaced=await evaluate('__cyrusCapture.scripts.generation');
  await action({action:'load_scene',name:scene});await ready(scene,replaced);
  await consoleOpen(false);await progress();
  await action({action:'place_player',x:point[0],y:point[1],z:point[2],heading:0});
  await resetCamera();
  await consoleOpen(false);
  // A dormant room resumes deferred initialization before its ambient poll.
  // Admit actual new output while ordinary updates run, then inhibit for readback.
  const resumed=await evaluate('__sessionSamples.status().now');
  const resumedClock=await evaluate('__cyrusStartup.clock.game_seconds');
  await until('dormant ambient continuation',`(()=>{
    if(__cyrusStartup.clock.game_seconds>${resumedClock+10})throw Error('dormant ambient exceeded game-clock budget');
    return __sessionSamples.status().buffers.some(b=>b.frame>=${resumed}&&b.energy>.001);
  })()`,90);
  await consoleOpen(true);await record('revisited',true);
  writeFileSync(`${output}/result.json`,JSON.stringify(results,null,2));
}catch(error){
  writeFileSync(`${output}/failure.json`,JSON.stringify({error:String(error),results,observation:await evaluate('({startup:window.__cyrusStartup,capture:window.__cyrusCapture})').catch(()=>null),events:cdp.events},null,2));throw error;
}finally{provenance?.stop();await cdp.close();}
