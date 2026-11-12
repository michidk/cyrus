// Authored ambient polling waits for bank admission without replaying missed polls.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync,renameSync,openSync,fsyncSync,closeSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
import {checkLevels,expected} from './test-positional-pcm.mjs';
import {observeBrowserBuild} from './browser-provenance.mjs';
const [endpoint,page,install,output,emitter,effect,scene='ISLAND',destination='START',sourceFixture,emitterIndex,acceptancePhase='bank',listenerOffset='[0,0,0]'] = process.argv.slice(2);
assert(endpoint&&page&&install&&output&&emitter&&effect&&sourceFixture&&emitterIndex!==undefined,
  'usage: CDP_URL PAGE_URL INSTALL_URL OUTPUT EMITTER LOOP_EFFECT SCENE DESTINATION SOURCE_FIXTURE EMITTER_INDEX');
mkdirSync(output,{recursive:true});
const offset=JSON.parse(listenerOffset);
assert(offset.length===3&&offset.every(Number.isInteger),'invalid listener fixture offset');
const source=JSON.parse(readFileSync(sourceFixture));
assert(Number.isInteger(Number(emitterIndex))&&Number(emitterIndex)>=0,'invalid installed actor index');
assert(source.frames>0&&source.rate>0&&source.source_rms>0,'source fixture requires nonempty PCM period and RMS');
const url=new URL(page);url.search=new URLSearchParams({capture:'1',scene,install:new URL(install).href});
const cdp=await connectCdp(endpoint);let provenance,id=0;
assert(acceptancePhase==='bank','ambient browser driver supports the bank phase');
const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
const results={phase:acceptancePhase,driver_sha256:digest(readFileSync(new URL(import.meta.url))),
  source_fixture_sha256:digest(readFileSync(sourceFixture)),stages:{}};
function persist(name,value){
  const path=`${output}/${name}.json`,temporary=path+'.tmp';
  writeFileSync(temporary,JSON.stringify(value,null,2));
  const fd=openSync(temporary,'r');try{fsyncSync(fd)}finally{closeSync(fd)}
  renameSync(temporary,path);
  const directory=openSync(output,'r');try{fsyncSync(directory)}finally{closeSync(directory)}
}
async function finishPhase(name){
  results.provenance=await provenance.verify();
  persist(name+'-complete',results);
}
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
async function session(type,fields={}){
  const deadline=performance.now()+90000;
  for(;;){
    const current=++id;
    await evaluate(`window.__cyrusSessionRequest=${JSON.stringify(JSON.stringify({type,id:current,...fields}))};true`);
    const result=await until(type,`(()=>{
      const result=window.__cyrusSessionResult;
      if(result?.id!==${current})return null;
      if(!result.document)return result;
      // Inspect ownership in-page. Copying the full rendered-world document
      // over CDP for every readiness poll makes recovery unnecessarily slow.
      const {document,...metadata}=result;
      return {...metadata,sample_slots:JSON.parse(document.split('\\n').slice(2).join('\\n')).samples.ownership.samples.slots.filter(Boolean)};
    })()`);
    if(!result.pending)return result;
    assert(performance.now()<deadline,'session completion deadline');
    type='result';fields={};await delay(100);
  }
}
async function record(name,present){
  if(present){
    const deadline=performance.now()+90000;
    const initial=await evaluate('__cyrusStartup.clock.game_seconds');
    for(;;){await sample('inspect');
      const playing=await evaluate('__cyrusSampleInspection.playing');
      if(playing===1){
        await consoleOpen(true);
        if(await evaluate('__cyrusSampleInspection.bank_pending'))break;
        const captured=await session('capture');
        if(!captured.error){
          const slots=captured.sample_slots;
          if(slots.length===1 && slots[0].effect===Number(effect)
            && slots[0].owner?.Script?.index===Number(emitterIndex))break;
        }else assert(/model.*(not ready|settled)|export still in flight/.test(captured.error),captured.error);
      }
      assert(performance.now()<deadline && await evaluate(`__cyrusStartup.clock.game_seconds<${initial+10}`),
        'ambient admission exceeded bounded ordinary game-clock progress');
      // A single short progress interval need not reach the authored task's
      // next poll after admission or world return. Keep ordinary updates live.
      await consoleOpen(playing>1);await delay(100);
    }
    await consoleOpen(true);
  }
  await until('graph ready','window.__sessionSamples?.status().ready');
  const pcm=await evaluate(`__cyrusSoundReset.sample(${JSON.stringify(name)},${present})`);
  const state=await evaluate('({startup:__cyrusStartup,world:__cyrusCapture.world,scripts:__cyrusCapture.scripts,preferences:window.__cyrusMenu?.preferences})');
  const samples=await evaluate(`__sessionSamples.blocks.filter(b=>b.frame>=${pcm.start}&&b.frame<${pcm.end}).map(b=>({left:Array.from(b.left),right:Array.from(b.right)}))`);
  writeFileSync(`${output}/${name}-pcm.json`,JSON.stringify(samples));
  let positional,sampleOwner;
  if(source&&present){
    const actor=state.scripts.actors.find(row=>row.name===emitter&&row.id.index===Number(emitterIndex));
    const camera=state.startup.camera_pose;
    await sample('inspect');
    const inspection=await evaluate('structuredClone(__cyrusSampleInspection)');
    assert.equal(inspection.playing,1,'PCM oracle requires one isolated installed emitter');
    if(inspection.bank_pending){
      sampleOwner=results.stages['authored-before-bank']?.positional?.sampleOwner;
      assert(results.stages['authored-before-bank']?.positional,
        'pending-bank PCM requires a previously verified live emitter');
    }else{
      const deadline=performance.now()+90000;let captured;
      for(;;){captured=await session('capture');if(!captured.error)break;
        assert(/model.*(not ready|settled)|export still in flight/.test(captured.error)&&performance.now()<deadline,captured.error);await delay(250);}
      const slots=captured.sample_slots;
      assert.equal(slots.length,1,'PCM oracle requires one isolated installed emitter');
      assert.deepEqual(slots[0].owner,{Script:{generation:actor.id.generation,index:actor.id.index}},'audible transport belongs to another emitter');
      assert.equal(slots[0].effect,Number(effect),'audible transport uses another installed effect');
      sampleOwner=slots[0];
    }
    const setting=name=>Number(source.system.match(new RegExp(`^${name}=(.*)$`,'mi'))[1]);
    const stage={phase:name,position:(actor.spatial.effective?.position??actor.spatial.linked_position??actor.spatial.local.position),listener:camera.position,
      yaw:camera.angles[1],sound_distance:setting('sound_distance'),
      master:Math.trunc(results.preferences.values[601]*64/5),
      bits:source.bits,source_rms:source.source_rms};
    // Authored wind is nonstationary. Its full-file RMS cannot predict a
    // short arbitrary graph excerpt. Check gain over three complete source
    // periods of newly submitted mixer PCM, and pan in actual graph output.
    const fence=await evaluate('__sessionSamples.buffers.length');
    const duration=source.frames/source.rate;
    const submitted=await until('three source periods',`(()=>{
      const rows=__sessionSamples.buffers.slice(${fence});
      const frames=rows.reduce((sum,b)=>sum+b.left.length,0);
      if(frames<__sessionSamples.clock().rate*${duration*3})return null;
      return {frames,rms:['left','right'].map(key=>Math.sqrt(rows.reduce((sum,b)=>sum+b[key].reduce((s,v)=>s+v*v,0),0)/frames))};
    })()`,60);
    const levels=checkLevels(stage,submitted.rms,submitted.frames);
    const rms=channel=>Math.sqrt(samples.flatMap(b=>b[channel]).reduce((sum,v)=>sum+v*v,0)/(samples.length*128));
    const graphRms=[rms('left'),rms('right')],gain=expected(stage);
    assert(Math.abs(graphRms[0]/graphRms[1]-gain[0]/gain[1])<.015,'graph pan differs from installed positional rule');
    positional={stage,levels,graphRms,sampleOwner};
  }
  results.stages[name]={pcm,state,positional};
  results.provenance=await provenance.verify();
  persist(name+'-complete',{...results,stages:{[name]:results.stages[name]}});
  persist('progress',results);
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
  // Direct scene launch need not publish book state. Observe the live Sound
  // preference through ordinary book input before using it as an oracle input.
  async function bookKey(){
    await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await delay(100);
    await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  }
  await bookKey();await until('public sound preference','window.__cyrusMenu?.ready&&__cyrusMenu.preferences?.values[601]!==undefined',180);
  results.preferences=await evaluate('structuredClone(__cyrusMenu.preferences)');
  await bookKey();await ready(scene);await progress();
  let actor;
  for(let offset=0;offset<2048;offset+=64){
    await evaluate(`window.__cyrusInspectScriptsOffset=${offset};true`);
    const rows=await until('actor observation',`__cyrusCapture.scripts?.actors?.[0]?.id.index===${offset}&&__cyrusCapture.scripts.actors`);
    actor=rows.find(row=>row.name===emitter&&row.id.index===Number(emitterIndex));if(actor)break;
    if(rows.length<64)break;
  }
  assert(actor,'selected installed emitter absent');
  writeFileSync(`${output}/emitter.json`,JSON.stringify(actor,null,2));
  // Attached actors acquire an effective pose during ordinary family admission.
  // Their authored home is public placement data, unlike attachment-local offsets.
  const home=actor.spatial.effective?.position??actor.spatial.linked_position??actor.spatial.local.position;
  assert(home?.length===3,'selected emitter has no public home placement');
  let point=home.map(v=>Math.floor(v/256));
  await action({action:'place_player',x:point[0]+512+offset[0],y:point[1]+offset[1],z:point[2]+offset[2],heading:0});
  await resetCamera();
  await consoleOpen(false);await progress();
  if(actor.spatial.parent!==null){
    const admitted=await until('attached emitter family admission',
      `__cyrusCapture.scripts.actors.find(row=>row.name===${JSON.stringify(emitter)}&&row.id.index===${Number(emitterIndex)})?.spatial.effective`);
    point=admitted.position.map(v=>Math.floor(v/256));
  }
  await consoleOpen(false);await progress();await consoleOpen(true);
  // Retire incidental entry one-shots, then let the authored ambient poll
  // supply the isolated continuing source. No diagnostic tone is injected.
  await sample('stop');
  await consoleOpen(false);await progress();await consoleOpen(true);
  await record('authored-before-bank',true);
  await evaluate(`(()=>{const read=cyrusGameAssets.read.bind(cyrusGameAssets);window.__bankControl={hold:true,waiting:false};cyrusGameAssets.read=async path=>{if(path==='sounds/bank.sounds'&&__bankControl.hold){__bankControl.waiting=true;await new Promise(resolve=>__bankControl.release=resolve)}return read(path)};return true})()`);
  await sample('reload_bank');
  await until('real bank read pending','__bankControl.waiting&&__cyrusSampleInspection.bank_pending');
  await action({action:'place_player',x:point[0]-512+offset[0],y:point[1]+offset[1],z:point[2]+offset[2],heading:0});
  await resetCamera();await progress();await consoleOpen(true);
  await record('pending-live-gain-update',true);
  await sample('stop');
  await consoleOpen(false);await progress();await consoleOpen(true);
  await record('pending-polls-stay-silent',false);
  await evaluate('__bankControl.hold=false;__bankControl.release();true');
  const admissionDeadline=performance.now()+30000;
  for(;;){await sample('inspect');if(await evaluate('!__cyrusSampleInspection.bank_pending'))break;assert(performance.now()<admissionDeadline,'bank admission deadline');await delay(100);}
  await consoleOpen(false);await progress();await consoleOpen(true);
  await record('admitted-next-poll',true);
  const admittedOwner=results.stages['admitted-next-poll'].positional.sampleOwner;
  assert.notEqual(admittedOwner.token,results.stages['authored-before-bank'].positional.sampleOwner.token,
    'stopped old-bank owner survived fresh admission');
  await consoleOpen(false);await progress();await consoleOpen(true);
  // Other authored one-shots may start on these updates. The capture readiness
  // waits for their retirement, then validates the selected ambient owner.
  await record('continued-once',true);
  assert.deepEqual(results.stages['continued-once'].positional.sampleOwner,admittedOwner,
    'ordinary polling duplicated or restarted the admitted ambient owner');
  await finishPhase('bank');
  persist('result',results);
}catch(error){
  results.failureCapture=await session('capture').catch(e=>({error:String(e)}));
  writeFileSync(`${output}/failure.json`,JSON.stringify({error:String(error),results,observation:await evaluate('({startup:window.__cyrusStartup,capture:window.__cyrusCapture,samples:window.__cyrusSampleInspection})').catch(()=>null),events:cdp.events},null,2));throw error;
}finally{provenance?.stop();await cdp.close();}
