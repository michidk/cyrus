// Installed authored conversation, public input and scene replacement, actual graph PCM.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {pathToFileURL} from 'node:url';
import {connectCdp} from './capture-cdp.mjs';
import {observeBrowserBuild} from './browser-provenance.mjs';
import {captureSampleGraph} from './capture-sample-graph.mjs';

export function validateObservation(row, generation, cleared = false) {
  assert.equal(row.generation,generation,'speech observation belongs to a different world generation');
  assert(row.dialogue,'dialogue observation unavailable');
  for(const record of row.dialogue.records){
    if(record.active) assert.equal(record.owner?.generation,generation,'active speech retains a retired speaker');
    if(cleared){assert.equal(record.active,false,'world replacement retained active dialogue');assert.equal(record.sample,null,'world replacement retained a sample handle');assert.equal(record.text,'','world replacement retained subtitle text');}
  }
  if(cleared) assert.equal(row.dialogue.presented,null,'world replacement retained presentation');
}
export function activeVoice(snapshot,row){
  const record=row.dialogue.records[row.dialogue.presented];
  assert(record?.active&&record.sample!==null,'active authored voice required');
  const slot=snapshot.samples.ownership.samples.slots[record.sample];
  const player=snapshot.samples.players.find(player=>player.token===slot?.token);
  assert(player,'active subtitle has no admitted playback transport');
  assert.equal(slot.effect,null,'selected subtitle sample belongs to an effect');
  return player;
}
export function validateRetirement(previous,replacement,voice){
  const retired=new Set(previous.players.map(player=>player.entity));
  assert(replacement.players.every(player=>!retired.has(player.entity)),'replacement retains an old playback entity');
  assert(replacement.players.every(player=>player.transport.source_identity!==voice.transport.source_identity),'replacement retained the interrupted voice source');
}
export function requireSecureOrigin(page){
  const url=new URL(page);
  assert(url.protocol==='https:' || (url.protocol==='http:'&&['127.0.0.1','localhost','[::1]'].includes(url.hostname)),
    'PCM capture requires HTTPS or a loopback relay; insecure-origin flags do not establish AudioWorklet availability');
}

async function main(){
  const [endpoint,page,install,fixturePath,output]=process.argv.slice(2);
  assert(endpoint&&page&&install&&fixturePath&&output,'usage: CDP_URL PAGE_URL INSTALL_URL FIXTURE_JSON OUTPUT');
  requireSecureOrigin(page);
  const fixture=JSON.parse(readFileSync(fixturePath));
  assert(fixture.scene&&fixture.destination&&fixture.scene!==fixture.destination,'distinct installed worlds required');
  assert(Number.isInteger(fixture.minimum_lines)&&fixture.minimum_lines>=2,'installed conversation line count required');
  mkdirSync(output,{recursive:true});
  assert(!['result.json','progress.json','failure.json'].some(name=>{try{readFileSync(`${output}/${name}`);return true;}catch(e){if(e.code==='ENOENT')return false;throw e;}}),'prior run receipts cannot be reused');
  const url=new URL(page);url.search=new URLSearchParams({capture:'1',scene:fixture.scene,install:new URL(install).href});
  const cdp=await connectCdp(endpoint);let provenance,id=0;
  const result={stages:{},fixture};
  async function evaluate(expression){const {result,exceptionDetails}=await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});assert(!exceptionDetails,result.description??exceptionDetails?.text);return result.value;}
  async function until(label,expression,seconds=90){const deadline=performance.now()+seconds*1000;while(performance.now()<deadline){const value=await evaluate(expression);if(value)return value;await delay(100);}throw Error(`bounded operation expired: ${label}`);}
  async function observe(){return evaluate(`(()=>{const s=__cyrusCapture.scripts;return {generation:s.generation,clock:__cyrusStartup.clock.game_seconds,dialogue:s.globals.dialogue,lockout:s.globals.lockout,menu:s.globals.menu,graph:__sessionSamples.clock()}})()`);}
  async function action(request){const current=++id;await evaluate(`window.__cyrusAcceptanceRequest=${JSON.stringify(JSON.stringify({...request,id:current}))};true`);const response=await until('public action',`__cyrusAcceptanceResult?.id===${current}&&__cyrusAcceptanceResult`);assert(response.applied,response.error);}
  async function key(code,key,vk,pressed=true){await cdp.call('Input.dispatchKeyEvent',{type:pressed?'keyDown':'keyUp',code,key,windowsVirtualKeyCode:vk});}
  async function tap(code,name,vk){await key(code,name,vk);try{const start=(await observe()).clock;await until('input sampled',`__cyrusStartup.clock.game_seconds>=${start+.25}`);}finally{await key(code,name,vk,false);}}
  async function ready(scene,generation){await until('prepared '+scene,`window.__cyrusCapture?.ready&&__cyrusCapture.world===${JSON.stringify(scene)}&&__cyrusCapture.scripts&&${generation===undefined?'true':`__cyrusCapture.scripts.generation!==${generation}`}`,600);}
  async function line(){await until('authored speech',`(()=>{const d=__cyrusCapture.scripts.globals.dialogue;return d.presented!==null&&d.records[d.presented]?.active&&d.records[d.presented].sample!==null})()`);return observe();}
  async function pcm(name,present){await until('graph ready','__sessionSamples.status().ready');const value=await evaluate(`__cyrusSoundReset.sample(${JSON.stringify(name)},${present})`);const state=await observe();validateObservation(state,state.generation);result.stages[name]={pcm:value,state,capture:await captureSampleGraph(cdp,`${output}/${name}`,value.start,value.end)};writeFileSync(`${output}/progress.json`,JSON.stringify(result,null,2));return state;}
  async function capture(){
    async function send(type){const current=++id;await evaluate(`window.__cyrusSessionRequest=${JSON.stringify(JSON.stringify({type,id:current}))};true`);return until('public capture response',`window.__cyrusSessionResult?.id===${current}&&__cyrusSessionResult`);}
    let response=await send('capture');const deadline=performance.now()+90000;
    while(response.pending){assert(performance.now()<deadline,'public capture completion deadline');await delay(100);response=await send('result');}
    assert(!response.error,response.error);return JSON.parse(response.document.split('\n').slice(2).join('\n'));
  }
  async function screenshot(name){writeFileSync(`${output}/${name}.png`,Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));}
  try{
    provenance=await observeBrowserBuild(cdp,JSON.parse(readFileSync('target/web/build-provenance.json')),url);
    await cdp.call('Page.enable');await cdp.call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});
    await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:readFileSync(new URL('./session-sample-graph.js',import.meta.url),'utf8')+'\n'+readFileSync(new URL('./browser-sound-reset.js',import.meta.url),'utf8')+`\nwindow.__cyrusInspectScripts=true;window.__speechLines=[];setInterval(()=>{const s=window.__cyrusCapture?.scripts,d=s?.globals.dialogue,r=d?.records[d.presented];if(r?.active){const last=__speechLines.at(-1);if(!last||last.generation!==s.generation||last.text!==r.text)__speechLines.push({generation:s.generation,clock:__cyrusStartup.clock.game_seconds,...r});}},50);`});
    await cdp.call('Page.navigate',{url:url.href});
    await until('explicit picker','document.getElementById("install-picker")');
    assert(await evaluate('isSecureContext&&("audioWorklet" in AudioContext.prototype)'),'production PCM capture has no secure AudioWorklet');
    assert.equal(await evaluate('document.getElementById("install-url").value'),new URL(install).href);
    await evaluate('document.getElementById("install-picker").requestSubmit();true');
    await ready(fixture.scene);
    const placementDeadline=performance.now()+90000;
    for(;;){try{await action({action:'place_player',...fixture.placement});break;}catch(error){assert(/spatial state unavailable/.test(String(error))&&performance.now()<placementDeadline,String(error));await delay(250);}}
    await action({action:'reset_camera'});
    await line();await pcm('greeting',true);await screenshot('greeting');
    await until('greeting completion','__cyrusCapture.scripts.globals.dialogue.records.every(r=>!r.active)');
    await evaluate('__speechLines=[];true');
    await tap('ControlLeft','Control',17);
    const start=(await observe()).clock;
    await until('authored exchange and topic menu',`(()=>{if(__cyrusStartup.clock.game_seconds>${start+45})throw Error('conversation game-clock budget');const g=__cyrusCapture.scripts.globals;return g.menu.count>0&&__speechLines.length>=${fixture.minimum_lines}})()`,180);
    result.conversation=await evaluate('__speechLines');
    assert(new Set(result.conversation.map(r=>r.owner.index)).size>=2,'exchange did not include both speakers');
    await screenshot('topic-menu');
    await tap('ControlLeft','Control',17);const interrupted=await line();
    await pcm('topic-speech',true);
    const playing=await capture();result.interrupted_samples=playing.samples;
    const oldVoice=activeVoice(playing,await observe());result.interrupted_voice=oldVoice;
    const beforeSkip=await observe();
    assert.equal(beforeSkip.lockout,1,'authored topic must own the skip lockout');
    assert(beforeSkip.dialogue.records.some(r=>r.active&&r.text===interrupted.dialogue.records[interrupted.dialogue.presented].text),'interruption fixture finished before the skip edge');
    await tap('ShiftLeft','Shift',16);
    result.skip=await observe();
    assert(!result.skip.dialogue.records.some(r=>r.active&&r.text===interrupted.dialogue.records[interrupted.dialogue.presented].text),'skip retained the interrupted line');
    // Readback after already mixed output drains is separate from the stop edge.
    const old=result.skip.generation;
    await action({action:'load_scene',name:fixture.destination});await ready(fixture.destination,old);
    const replacement=await observe();validateObservation(replacement,replacement.generation,true);
    result.replacement=replacement;await screenshot('replacement');
    const replaced=await capture();
    validateRetirement(playing.samples,replaced.samples,oldVoice);
    result.replacement_samples=replaced.samples;
    // New-world ambience is valid output. Retired dialogue ownership must not
    // be confused with requiring silence from every new-world source.
    await until('replacement graph progress','__sessionSamples.status().ready');
    result.replacement_graph=await evaluate(`(async()=>{const g=__sessionSamples,s=g.status(),start=Math.ceil(Math.max(s.now,...s.buffers.map(b=>b.frame+b.length))/128)*128+128,end=start+8192;const deadline=performance.now()+15000;while(g.status().last<end){if(performance.now()>deadline)throw Error('replacement graph readback expired');await new Promise(r=>setTimeout(r,50));}return {start,end,...g.match(start,end)};})()`);
    result.replacement_capture=await captureSampleGraph(cdp,`${output}/replacement-output`,result.replacement_graph.start,result.replacement_graph.end);
    await action({action:'load_scene',name:fixture.scene});await ready(fixture.scene,replacement.generation);
    await action({action:'place_player',...fixture.placement});await action({action:'reset_camera'});
    await tap('ControlLeft','Control',17);const freshLine=await line();const fresh=await pcm('fresh-speech',true);
    result.fresh_voice=activeVoice(await capture(),freshLine);
    assert.notEqual(result.fresh_voice.entity,oldVoice.entity,'fresh speech reused retired playback identity');
    assert.notEqual(fresh.generation,old);await screenshot('fresh-speech');
    result.provenance=await provenance.verify();writeFileSync(`${output}/result.json`,JSON.stringify(result,null,2));
  }catch(error){writeFileSync(`${output}/failure.json`,JSON.stringify({error:String(error),result,observation:await observe().catch(()=>null),events:cdp.events},null,2));throw error;}finally{provenance?.stop();await cdp.close();}
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href)await main();
