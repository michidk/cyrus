// Exercise installed movie exit with real keys; never replace codec data or simulation time.
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { connectCdp, captureStoppedFrame, stopAtWorldEndpoint } from './capture-cdp.mjs';
import { observeBrowserBuild } from './browser-provenance.mjs';
const [endpoint, page, install, output] = process.argv.slice(2);
assert(endpoint && page && install && output, 'usage: node dev/scripts/test-browser-movie-handoff.mjs CDP_URL PAGE_URL INSTALL_URL OUTPUT');
mkdirSync(output, {recursive:true});
const build = JSON.parse(readFileSync('target/web/build-provenance.json'));
const cdp = await connectCdp(endpoint), url = new URL(page);
url.search = new URLSearchParams({install:new URL(install).href,capture:'1'});
let provenance, verifiedBuild, failure, definition, destination, frozenEndpoint, previous, lastKeyFrame = -1, movieSeen = false, exited = false, done = false;
const samples = [], keys = [], shots = [], start = performance.now();
let deadline = start + 180000;
async function evaluate(expression) {
  const result = await cdp.call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true});
  assert(!result.exceptionDetails, result.result.description); return result.result.value;
}
async function shot(name) {
  const {data} = await cdp.call('Page.captureScreenshot',{format:'png'});
  writeFileSync(`${output}/${name}.png`,Buffer.from(data,'base64')); shots.push(name);
}
try {
  provenance = await observeBrowserBuild(cdp,build,url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});
  // Stop only this disposable test page after the first completed world update.
  // Readback must not run another simulation callback while the GPU returns pixels.
  await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:`(${stopAtWorldEndpoint.toString()})()`});
  await cdp.call('Page.navigate',{url:url.href});
  while(performance.now() < deadline && performance.now() - start < 300000) {
    const state = await evaluate(`(() => {const p=document.getElementById('install-picker');if(p&&!p.dataset.submitted){p.dataset.submitted='1';p.requestSubmit();}window.__cyrusInspectScripts=true;return {startup:window.__cyrusStartup,menu:window.__cyrusStartup?.state==='Menu'?window.__cyrusMenu:null,world:window.__cyrusCapture?{ready:window.__cyrusCapture.ready,world:window.__cyrusCapture.world,frame:window.__cyrusCapture.scripts?.globals?.world_loop_frame}:null,error:document.getElementById('failure')?.textContent};})()`);
    samples.push({wall:(performance.now()-start)/1000,...state});
    writeFileSync(`${output}/progress.json`, JSON.stringify(samples.at(-1))+'\n');
    assert(!state.error,state.error);
    if(movieSeen&&state.startup?.state==='Loading') assert.equal(state.startup.clock.game_seconds,0,'world clock advanced before gameplay');
    const movie = state.menu?.movie;
    if(state.startup?.state==='InGame'&&state.world?.ready) {
      assert(movieSeen&&exited,'world bypassed movie exit');
      if(!await evaluate('window.__cyrusEndpointStopped===true')){await delay(20);continue;}
      const capture = await captureStoppedFrame(cdp, `({startup:window.__cyrusStartup,world:window.__cyrusCapture.world,frame:window.__cyrusCapture.scripts.globals.world_loop_frame})`);
      frozenEndpoint=capture.observation;
      assert.equal(frozenEndpoint.world,String(destination.world),'wrong installed startup destination');
      assert.equal(frozenEndpoint.startup.state,'InGame','startup presentation did not hand off to gameplay');
      assert(frozenEndpoint.startup.clock.game_seconds>0,'world clock did not start');
      assert(frozenEndpoint.frame>0&&frozenEndpoint.frame<=4,'first rendered frame capture overshot its update cutoff');
      writeFileSync(`${output}/first-rendered-world.png`,Buffer.from(capture.data,'base64'));shots.push('first-rendered-world');
      done=true;break;
    }
    const event = JSON.stringify([state.startup?.state,!!movie,movie?.loading,state.world?.ready]);
    if(event!==previous){await shot(`transition-${shots.length}`);previous=event;}
    if(movie?.frame>0&&!movie.loading) {
      assert.equal(movie.name,'INTRO.SMK'); assert.equal(state.startup.clock.game_seconds,0,'world clock advanced during Introduction');
      if(!movieSeen){
        definition=await evaluate(`window.cyrusGameAssets.read('movies/0.movie').then(b=>{const m=JSON.parse(new TextDecoder().decode(b));return {keys:m.definition.keys,frames:m.frames,frame_micros:m.frame_micros};})`);
        destination=await evaluate(`window.cyrusGameAssets.read('menu/book.menu').then(b=>{const {world,marker}=JSON.parse(new TextDecoder().decode(b)).new_game;return {world,marker};})`);
        verifiedBuild=await provenance.verify();
        // Debugging WASM inhibits optimized execution; loaded bytes are recorded already.
        await cdp.call('Debugger.disable');
        movieSeen=true;await shot('movie-before-skips');
      }
      if(movie.frame>lastKeyFrame) {
        const target=definition.keys.find(frame=>frame>movie.frame)??null;
        keys.push({frame:movie.frame,target,wall:(performance.now()-start)/1000});
        await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        await delay(100);
        await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        lastKeyFrame=target??Infinity;deadline=performance.now()+90000;
      }
    }
    if(movieSeen&&!movie&&!exited){exited=true;await shot('movie-exit');deadline=performance.now()+90000;}
    assert(!state.menu?.ready,'movie exit returned to the startup book');
    await delay(100);
  }
  assert(done,'movie-to-world endpoint deadline');
  console.log(`handoff: passed; ${samples.length} samples, ${keys.length} public skip keys`);
}catch(error){failure=String(error.stack??error);throw error;}
finally{writeFileSync(`${output}/result.json`,JSON.stringify({failure,definition,destination,frozenEndpoint,verifiedBuild,samples,keys,shots},null,2)+'\n');provenance?.stop();await cdp.close();}
