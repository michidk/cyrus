// Hold real destination reads to separate the load request from publication.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync,existsSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
import {observeBrowserBuild} from './browser-provenance.mjs';
import {requireSecureOrigin} from './browser-authored-speech.mjs';
const [endpoint,page,install,fixturePath,output]=process.argv.slice(2);
assert(endpoint&&page&&install&&fixturePath&&output,'usage: CDP PAGE INSTALL FIXTURE OUTPUT');
requireSecureOrigin(page);mkdirSync(output,{recursive:true});
assert(!['result.json','failure.json'].some(file=>existsSync(`${output}/${file}`)),'prior music receipts cannot be reused');
const fixture=JSON.parse(readFileSync(fixturePath));
assert(fixture.scene&&fixture.destination&&fixture.alias,'installed source, alias and different-track destination required');
const url=new URL(page);url.search=new URLSearchParams({capture:'1',scene:fixture.scene,install:new URL(install).href});
const cdp=await connectCdp(endpoint);let provenance,id=0;const result={fixture};
async function evaluate(expression){const {result,exceptionDetails}=await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});assert(!exceptionDetails,result.description??exceptionDetails?.text);return result.value;}
async function until(label,expression,seconds=90){const deadline=performance.now()+seconds*1000;while(performance.now()<deadline){const value=await evaluate(expression);if(value)return value;await delay(100);}throw Error(`bounded music acceptance expired: ${label}`);}
async function action(name){const current=++id;await evaluate(`window.__cyrusAcceptanceRequest=${JSON.stringify(JSON.stringify({id:current,action:'load_scene',name}))};true`);const response=await until('scene request',`window.__cyrusAcceptanceResult?.id===${current}&&__cyrusAcceptanceResult`);assert(response.applied,response.error);}
async function session(type,fields={}){
  async function send(type,fields={}){const current=++id;await evaluate(`window.__cyrusSessionRequest=${JSON.stringify(JSON.stringify({id:current,type,...fields}))};true`);return until('public session response',`window.__cyrusSessionResult?.id===${current}&&__cyrusSessionResult`);}
  let response=await send(type,fields);const deadline=performance.now()+90000;
  while(response.pending){assert(performance.now()<deadline,'session publication deadline');await delay(100);response=await send('result');}
  assert(!response.error,response.error);return response;
}
async function musicOwner(){const response=await session('capture');return JSON.parse(response.document.split('\n').slice(2).join('\n')).music;}
async function key(code,vk){await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',code,key:code,windowsVirtualKeyCode:vk});await delay(100);await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',code,key:code,windowsVirtualKeyCode:vk});}
async function ready(previous){await until('new prepared world',`window.__cyrusCapture?.ready&&__cyrusCapture.scripts&&${previous===undefined?'true':`__cyrusCapture.scripts.generation!==${previous}`}`,600);}
async function outputWindow(index,present,label){
  const observation=await evaluate(`(async()=>{
    const p=__sessionCd,s=p.sources[${index}],rate=s.rate;
    const start=Math.ceil(s.gain.context.currentTime*rate/128)*128+256,end=start+8192,deadline=performance.now()+15000;
    while((p.blocks.at(-1)?.frame??0)<end){if(performance.now()>deadline)throw Error('CD output readback deadline');await new Promise(r=>setTimeout(r,50));}
    if(p.error)throw Error(p.error);
    const blocks=p.blocks.filter(b=>b.frame>=start&&b.frame<end);
    if(blocks.length!==64||blocks.some((b,i)=>b.frame!==start+i*128))throw Error('missing or duplicate CD quantum');
    let energy=0;for(const b of blocks)for(const c of [b.left,b.right])for(const x of c)energy+=x*x;
    if(${present}&&energy<=.01)throw Error('CD presence window is silent');
    if(!${present}&&energy>1e-12)throw Error('retired CD source still reaches the graph');
    let best=Infinity,bestSkew;
    if(${present})for(let skew=-128;skew<=128;skew++){
      let error=0;for(const b of blocks)for(let channel=0;channel<2;channel++){
        const reference=s.buffer.getChannelData(channel),actual=channel?b.right:b.left;
        for(let i=0;i<128;i++){const at=((Math.round((b.frame+i-s.frame)*s.buffer.sampleRate/rate+s.offset*s.buffer.sampleRate)+skew)%reference.length+reference.length)%reference.length;error+=(actual[i]-reference[at]*s.gain.gain.value)**2;}
      }
      if(error<best){best=error;bestSkew=skew;}
    }
    if(${present}&&best/energy>=1e-6)throw Error('CD graph differs from the installed source');
    return {pcm:blocks.flatMap(b=>Array.from(b.left).flatMap((sample,i)=>[sample,b.right[i]])),start,end,frames:end-start,energy,error:${present}?best:0,skew:bestSkew,status:p.status()};
  })()`);
  const bytes=Buffer.alloc(observation.pcm.length*4);
  observation.pcm.forEach((sample,i)=>bytes.writeFloatLE(sample,i*4));
  writeFileSync(`${output}/${label}.f32le`,bytes);
  delete observation.pcm;
  writeFileSync(`${output}/${label}.json`,JSON.stringify(observation,null,2));
  return observation;
}
try{
  provenance=await observeBrowserBuild(cdp,JSON.parse(readFileSync('target/web/build-provenance.json')),url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});
  await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:readFileSync(new URL('./session-cd-graph.js',import.meta.url),'utf8')+'\nwindow.__cyrusInspectScripts=true;'});
  await cdp.call('Page.navigate',{url:url.href});await until('picker','document.getElementById("install-picker")');
  assert.equal(await evaluate('document.getElementById("install-url").value'),new URL(install).href);
  await evaluate('document.getElementById("install-picker").requestSubmit();true');await ready();
  await until('installed CD output','__sessionCd.status().ready&&__sessionCd.sources.length===1',180);
  result.initial=await outputWindow(0,true,'initial');
  if(fixture.scenario==='restore'){
    result.initialMusic=await musicOwner();
    await key('F10',121);await until('credits CD admitted','__sessionCd.sources.length===2');
    result.credits=await outputWindow(1,true,'credits');
    await key('Escape',27);await until('credits source retired','__sessionCd.sources[1].stopped!==null');
    const frame=await evaluate('__cyrusCapture.scripts.globals.world_loop_frame');
    await until('world clock resumes',`__cyrusCapture.scripts.globals.world_loop_frame>${frame}+2`);
    result.savedMusic=await musicOwner();assert.equal(result.savedMusic.track,null,'fixture must have stopped CD after credits');
    await session('save',{key:'stopped-cd'});await session('load',{key:'stopped-cd'});
    await evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
    result.restoredMusic=await musicOwner();
    if(result.restoredMusic.track!==null)result.unexpected=await outputWindow(await evaluate('__sessionCd.sources.length-1'),true,'unexpected-restored');
    assert.equal(result.restoredMusic.track,null,'diagnostic restoration restarted world music instead of preserving stopped ownership');
    result.restored=await outputWindow(1,false,'restored');
    const generation=await evaluate('__cyrusCapture.scripts.generation');
    const count=await evaluate('__sessionCd.sources.length');
    await evaluate(`(()=>{const read=cyrusGameAssets.read.bind(cyrusGameAssets);window.__superseded={read,returned:0};cyrusGameAssets.read=async path=>{
      if(path.startsWith(${JSON.stringify(fixture.destination+'.')})){__superseded.pending=true;await new Promise(resolve=>(__superseded.release??=[]).push(resolve));const bytes=await read(path);__superseded.returned++;return bytes;}return read(path);
    };return true})()`);
    await action(fixture.destination);await until('superseded reads held','__superseded.pending');
    await action(fixture.alias);await ready(generation);
    await until('latest destination music',`__sessionCd.sources.length===${count+1}`);
    result.cancelled=await outputWindow(count,true,'latest-destination');
    const admitted=await evaluate('__cyrusCapture.scripts.generation');
    await evaluate('cyrusGameAssets.read=__superseded.read;__superseded.release.forEach(resolve=>resolve());true');
    await until('stale read completed','__superseded.returned>0');
    const settled=await evaluate('__cyrusCapture.scripts.globals.world_loop_frame');
    await until('latest world continues',`__cyrusCapture.scripts.globals.world_loop_frame>${settled}+3`);
    assert.equal(await evaluate('__cyrusCapture.scripts.generation'),admitted,'superseded destination replaced the admitted owner');
    assert.equal(await evaluate('__sessionCd.sources.length'),count+1,'superseded destination admitted stale music');
    result.afterCancellation=await musicOwner();assert.equal(result.afterCancellation.track,result.initialMusic.track);
    await session('save',{key:'active-cd'});await session('load',{key:'active-cd'});
    await until('active saved CD replacement',`__sessionCd.sources.length===${count+2}`);
    assert(await evaluate(`__sessionCd.sources[${count}].stopped!==null`),'active restore retained old output');
    result.activeRestored=await outputWindow(count+1,true,'active-restored');
    result.activeMusic=await musicOwner();assert.equal(result.activeMusic.track,result.initialMusic.track);
  }else{
  const originalGeneration=await evaluate('__cyrusCapture.scripts.generation');await action(fixture.alias);await ready(originalGeneration);
  assert(await evaluate('__sessionCd.sources.length===1&&__sessionCd.sources[0].stopped===null'),'same-track named entry restarted the CD');
  result.continued=await outputWindow(0,true,'continued');
  const generation=await evaluate('__cyrusCapture.scripts.generation');
  await evaluate(`(()=>{const read=cyrusGameAssets.read.bind(cyrusGameAssets);window.__heldWorld={pending:false,read};cyrusGameAssets.read=async path=>{if(path.startsWith(${JSON.stringify(fixture.destination+'.')})){__heldWorld.pending=true;await new Promise(resolve=>(__heldWorld.release??=[]).push(resolve));}return read(path)};return true})()`);
  await action(fixture.destination);await until('real destination asset read held','__heldWorld.pending');
  await evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
  assert.equal(await evaluate('__cyrusCapture.scripts.generation'),generation,'destination published before held asset admission');
  const stopped=await evaluate('__sessionCd.sources[0].stopped!==null');
  result.pending=await outputWindow(0,!stopped,'pending');
  assert(stopped,'named destination kept the old CD playing during real preparation');
  await evaluate('cyrusGameAssets.read=__heldWorld.read;__heldWorld.release.forEach(resolve=>resolve());true');
  await ready(generation);await until('replacement CD admitted','__sessionCd.sources.length===2');
  result.replacement=await outputWindow(1,true,'replacement');
  const destinationGeneration=await evaluate('__cyrusCapture.scripts.generation');
  await action(fixture.alias);await ready(destinationGeneration);
  await until('revisit CD admitted','__sessionCd.sources.length===3');
  assert(await evaluate('__sessionCd.sources[1].stopped!==null'),'revisit kept the destination CD source');
  result.revisit=await outputWindow(2,true,'revisit');
  const revisitGeneration=await evaluate('__cyrusCapture.scripts.generation');
  await action(fixture.alias);await ready(revisitGeneration);
  assert(await evaluate('__sessionCd.sources.length===3&&__sessionCd.sources[2].stopped===null'),'same-track revisit restarted the CD');
  result.revisitContinued=await outputWindow(2,true,'revisit-continued');
  result.trackBuffers=await evaluate(`(()=>{
    const buffers=__sessionCd.sources.map(source=>source.buffer);
    const equal=(a,b)=>a.length===b.length&&a.numberOfChannels===b.numberOfChannels&&
      Array.from({length:a.numberOfChannels},(_,channel)=>{
        const x=a.getChannelData(channel),y=b.getChannelData(channel);
        return x.every((sample,i)=>sample===y[i]);
      }).every(Boolean);
    return {destinationDiffers:!equal(buffers[0],buffers[1]),revisitMatches:equal(buffers[0],buffers[2]),lengths:buffers.map(buffer=>buffer.length)};
  })()`);
  assert(result.trackBuffers.destinationDiffers,'destination reused the old track buffer');
  assert(result.trackBuffers.revisitMatches,'revisit did not restore the installed source track');
  }
  result.provenance=await provenance.verify();
  writeFileSync(`${output}/result.json`,JSON.stringify(result,null,2));
}catch(error){writeFileSync(`${output}/failure.json`,JSON.stringify({error:String(error),result,status:await evaluate('window.__sessionCd?.status()').catch(()=>null),events:cdp.events},null,2));throw error;}finally{provenance?.stop();await cdp.close();}
