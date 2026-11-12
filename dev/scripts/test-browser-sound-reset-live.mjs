// Installed public book cancellation with actual CPAL graph output.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
import {observeBrowserBuild} from './browser-provenance.mjs';
const [endpoint, page, install, label, effect = '2', scene = '0'] = process.argv.slice(2);
assert(endpoint && page && install && label,
  'usage: node dev/scripts/test-browser-sound-reset-live.mjs CDP_URL PAGE_URL INSTALL_URL VOICE_LABEL [LOOP_EFFECT] [SCENE]');
const url = new URL(page);
url.search = new URLSearchParams({capture:'1', scene, install:new URL(install).href});
const cdp = await connectCdp(endpoint);
let provenance;
async function evaluate(expression) {
  const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {expression, returnByValue:true, awaitPromise:true});
  assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
  return result.value;
}
async function until(label, expression, seconds = 60) {
  const deadline = performance.now() + seconds * 1000;
  while (performance.now() < deadline) {
    const value = await evaluate(expression);
    if (value) return value;
    await delay(100);
  }
  throw Error(`bounded browser operation expired: ${label}`);
}
async function escape() {
  await cdp.call('Input.dispatchKeyEvent', {type:'keyDown', key:'Escape', code:'Escape', windowsVirtualKeyCode:27});
  await delay(250);
  await cdp.call('Input.dispatchKeyEvent', {type:'keyUp', key:'Escape', code:'Escape', windowsVirtualKeyCode:27});
}
async function start(id) {
  await evaluate(`window.__cyrusAudioRequest=JSON.stringify({id:${id},generation:__cyrusCapture.scripts.generation,index:0,label:${JSON.stringify(label)},effect:${Number(effect)}})`);
  const result = await until('sample start', `window.__cyrusAudioResult?.id===${id} && !window.__cyrusAudioResult.pending && window.__cyrusAudioResult`);
  assert.equal(result.started, true, result.error);
}
try {
  provenance = await observeBrowserBuild(cdp, JSON.parse(readFileSync('target/web/build-provenance.json')), url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride', {width:640,height:480,deviceScaleFactor:1,mobile:false});
  await cdp.call('Page.addScriptToEvaluateOnNewDocument', {source:
    readFileSync(new URL('./session-sample-graph.js', import.meta.url),'utf8') + '\n' +
    readFileSync(new URL('./browser-sound-reset.js', import.meta.url),'utf8')});
  await cdp.call('Page.navigate', {url:url.href});
  await until('installed gameplay', `(() => {
    const picker=document.getElementById('install-picker');
    if (picker && document.getElementById('install-url')?.value && !picker.dataset.soundSubmitted) {
      picker.dataset.soundSubmitted='true'; picker.requestSubmit();
    }
    window.__cyrusInspectScripts=true;
    return window.__cyrusCapture?.ready && window.__cyrusStartup?.clock?.game_seconds >= .4;
  })()`, 240);
  assert(await evaluate('isSecureContext && !!window.AudioWorkletNode'), 'AudioWorklet requires a secure browser origin');
  const initial = await evaluate('__cyrusStartup.clock.game_seconds');
  const point = await evaluate(`(() => {const r=document.querySelector('canvas').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  for (const type of ['mousePressed','mouseReleased']) await cdp.call('Input.dispatchMouseEvent',{type,button:'left',clickCount:1,...point});
  await start(1);
  await until('graph ready', '__sessionSamples.status().ready');
  const started = await evaluate(`__cyrusSoundReset.sample('started',true)`);
  await escape();
  await until('book ready', `__cyrusStartup.state==='Menu' && window.__cyrusMenu?.ready`);
  const book = await evaluate('({context:__cyrusMenu.context,page:__cyrusMenu.page,ready:__cyrusMenu.ready})');
  const stopped = await evaluate(`__cyrusSoundReset.sample('book-stopped',false)`);
  await escape();
  await until('continued gameplay', `__cyrusStartup.state==='InGame' && __cyrusCapture.ready`);
  await start(2);
  const continued = await evaluate(`__cyrusSoundReset.sample('continued',true)`);
  await escape();
  await until('final book ready', `__cyrusStartup.state==='Menu' && __cyrusMenu.ready`);
  const stoppedAgain = await evaluate(`__cyrusSoundReset.sample('continued-stopped',false)`);
  await escape();
  const final = await until('game-clock continuation', `__cyrusStartup.state==='InGame' && __cyrusStartup.clock.game_seconds >= ${initial + .4} && __cyrusStartup.clock.game_seconds`);
  console.log(JSON.stringify({viewport:{width:640,height:480},initial,final,book,started,stopped,continued,stoppedAgain,provenance:await provenance.verify()},null,2));
} catch (error) {
  console.error(JSON.stringify({events:cdp.events},null,2));
  throw error;
} finally {
  provenance?.stop();
  await cdp.close();
}
