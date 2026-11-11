// Bound installed New Game at an ordinary update, independent of host decoding speed.
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { connectCdp, captureStoppedFrame, stopAtWorldEndpoint } from './capture-cdp.mjs';
import { observeBrowserBuild } from './browser-provenance.mjs';
const [endpoint, page, install, output] = process.argv.slice(2);
assert(endpoint && page && install && output, 'usage: node dev/scripts/test-browser-world-entry.mjs CDP_URL PAGE_URL INSTALL_URL OUTPUT');
mkdirSync(output,{recursive:true});
const cdp=await connectCdp(endpoint), url=new URL(page);
url.search=new URLSearchParams({install:new URL(install).href,capture:'1','new-game':'true','skip-movies':'true'});
const samples=[];
let provenance,verifiedBuild,failure,destination,capture;
async function evaluate(expression){
  const result=await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  assert(!result.exceptionDetails,result.result.description);return result.result.value;
}
try {
  provenance=await observeBrowserBuild(cdp,JSON.parse(readFileSync('target/web/build-provenance.json')),url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});
  await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:`(${stopAtWorldEndpoint.toString()})(4)`});
  await cdp.call('Page.navigate',{url:url.href});
  const start=performance.now();
  while(performance.now()-start<90000){
    const state=await evaluate(`(()=>{const p=document.getElementById('install-picker');if(p&&!p.dataset.submitted){p.dataset.submitted='1';p.requestSubmit();}window.__cyrusInspectScripts=true;return {startup:window.__cyrusStartup,ready:window.__cyrusCapture?.ready,frame:window.__cyrusCapture?.scripts?.globals?.world_loop_frame,stopped:window.__cyrusEndpointStopped,error:document.getElementById('failure')?.textContent};})()`);
    samples.push({wall:(performance.now()-start)/1000,...state});
    assert(!state.error,state.error);
    if(state.stopped){
      verifiedBuild=await provenance.verify();
      await cdp.call('Debugger.disable');
      destination=await evaluate(`window.cyrusGameAssets.read('menu/book.menu').then(b=>{const {world,marker}=JSON.parse(new TextDecoder().decode(b)).new_game;return {world,marker};})`);
      capture=await captureStoppedFrame(cdp,`({startup:window.__cyrusStartup,world:window.__cyrusCapture.world,view:window.__cyrusCapture.view,frame:window.__cyrusCapture.scripts.globals.world_loop_frame,player:window.__cyrusCapture.scripts.actors.filter(a=>a.source==='player').map(a=>({id:a.id,position:a.spatial?.effective?.position,hidden:a.control?.hidden,animation:a.animation}))})`);
      writeFileSync(`${output}/world-update-4.png`,Buffer.from(capture.data,'base64'));
      const state=capture.observation;
      assert.equal(state.world,String(destination.world));
      assert.equal(state.frame,4,'endpoint overshot matched ordinary update');
      assert.equal(state.startup.state,'InGame');
      assert(state.startup.clock.game_seconds>0,'world clock did not start');
      assert(state.startup.camera_published,'first-world camera unavailable');
      assert(state.startup.world_fades.some(alpha=>alpha>0&&alpha<1),'first-world fade was consumed before ordinary gameplay');
      assert(state.player.some(player=>player.animation&&!player.hidden),'player model not prepared at matched update');
      break;
    }
    await delay(50);
  }
  assert(capture,'world-entry endpoint deadline');
  console.log('world entry: passed at ordinary update 4');
}catch(error){failure=String(error.stack??error);throw error;}
finally{
  writeFileSync(`${output}/result.json`,JSON.stringify({failure,destination,endpoint:capture?.observation,verifiedBuild,samples},null,2)+'\n');
  provenance?.stop();await cdp.close();
}
