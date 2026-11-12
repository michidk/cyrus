// Installed world-picker acceptance, using real input and a bounded loading endpoint.
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { connectCdp, captureStoppedFrame } from './capture-cdp.mjs';
import { observeBrowserBuild } from './browser-provenance.mjs';
const [endpoint, page, install, output] = process.argv.slice(2);
assert(endpoint && page && install && output, 'usage: node dev/scripts/test-browser-loading-artwork.mjs CDP_URL PAGE_URL INSTALL_URL OUTPUT');
mkdirSync(output, {recursive:true});
const cdp = await connectCdp(endpoint), url = new URL(page);
url.search = new URLSearchParams({install:new URL(install).href, capture:'1', 'new-game':'1', 'skip-movies':'1'});
let provenance, verifiedBuild, failure, result;
const samples = [], started = performance.now();
async function evaluate(expression) {
  const response = await cdp.call('Runtime.evaluate', {expression, returnByValue:true, awaitPromise:true});
  assert(!response.exceptionDetails, response.result.description); return response.result.value;
}
async function key(key, code, value) {
  await cdp.call('Input.dispatchKeyEvent', {type:'keyDown',key,code,windowsVirtualKeyCode:value});
  await delay(100);
  await cdp.call('Input.dispatchKeyEvent', {type:'keyUp',key,code,windowsVirtualKeyCode:value});
}
async function click(x,y) {
  await cdp.call('Input.dispatchMouseEvent', {type:'mouseMoved',x,y});
  await cdp.call('Input.dispatchMouseEvent', {type:'mousePressed',x,y,button:'left',clickCount:1});
  await delay(100);
  await cdp.call('Input.dispatchMouseEvent', {type:'mouseReleased',x,y,button:'left',clickCount:1});
  await delay(200);
}
async function shot(name) {
  const {data} = await cdp.call('Page.captureScreenshot',{format:'png'});
  writeFileSync(`${output}/${name}.png`,Buffer.from(data,'base64'));
}
try {
  const build = JSON.parse(readFileSync('target/web/build-provenance.json'));
  provenance = await observeBrowserBuild(cdp,build,url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});
  await cdp.call('Page.addScriptToEvaluateOnNewDocument',{source:`(() => {
    const schedule = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = callback => schedule(time => {
      if (window.__cyrusLoadingArmed && window.__cyrusStartup?.state === 'Loading'
          && window.__cyrusStartup.world_fades.length > 0
          && window.__cyrusStartup.world_fades.every(alpha => alpha <= 0.5)) {
        window.__cyrusLoadingStopped = true; return;
      }
      callback(time);
    });
  })()`});
  await cdp.call('Page.navigate',{url:url.href});
  let ready = false;
  while (performance.now()-started<180000) {
    const state = await evaluate(`(() => {const p=document.getElementById('install-picker');if(p&&!p.dataset.submitted){p.dataset.submitted='1';p.requestSubmit();}window.__cyrusInspectScripts=true;return {startup:window.__cyrusStartup,world:window.__cyrusCapture?.world,ready:window.__cyrusCapture?.ready,update:window.__cyrusCapture?.scripts?.globals?.world_loop_frame,error:document.getElementById('failure')?.textContent};})()`);
    samples.push(state); assert(!state.error,state.error);
    if(state.startup?.state==='InGame'&&state.ready&&state.update>=1){ready=true;break;}
    await delay(100);
  }
  assert(ready,'initial world preparation deadline');
  await key('F10','F10',121);
  await shot('picker');
  await click(80,84);
  await shot('choices');
  await click(80,151);
  await shot('selected');
  await evaluate('window.__cyrusLoadingArmed=true');
  await click(148,84);
  await key('F10','F10',121);
  const deadline = performance.now()+30000;
  while(performance.now()<deadline) {
    const state = await evaluate('({startup:window.__cyrusStartup,stopped:window.__cyrusLoadingStopped})');
    samples.push(state);
    if(state.stopped) {
      const capture = await captureStoppedFrame(cdp,'window.__cyrusStartup');
      result=capture.observation;
      writeFileSync(`${output}/loading.png`,Buffer.from(capture.data,'base64'));
      assert.equal(result.loading_world,2,'world picker presented artwork from another destination');
      assert.equal(result.state,'Loading');
      break;
    }
    await delay(50);
  }
  assert(result,'destination loading endpoint deadline');
  verifiedBuild=await provenance.verify();
  console.log('destination loading artwork: world2 presented through public world picker');
} catch(error) { failure=String(error.stack??error); throw error; }
finally { writeFileSync(`${output}/result.json`,JSON.stringify({failure,result,verifiedBuild,samples},null,2)+'\n');provenance?.stop();await cdp.close(); }
