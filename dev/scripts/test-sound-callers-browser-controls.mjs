// Real-browser negative controls for the independent SOUP output fixture.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint,insecure,secure] = process.argv.slice(2);
assert(endpoint && insecure && secure,'usage: node dev/scripts/test-sound-callers-browser-controls.mjs CDP_URL INSECURE_PAGE LOOPBACK_PAGE');
const cdp=await connectCdp(endpoint);
try {
  await cdp.call('Runtime.enable');
  await cdp.call('Page.enable');
  await cdp.call('Page.navigate',{url:insecure});
  const deadline=performance.now()+10000;
  let errors;
  do {
    errors=cdp.events.filter(e=>e.method==='Runtime.exceptionThrown').map(e=>e.params.exceptionDetails.exception?.description);
    if(errors.length)break;
    await delay(50);
  }while(performance.now()<deadline);
  assert(errors.some(e=>e?.includes('requires a secure context with AudioWorklet')),JSON.stringify(errors));
  console.log('insecure origin rejected before app startup');
}finally{await cdp.close()}
const wrongWasm=spawnSync(process.execPath,[fileURLToPath(new URL('./test-sound-callers-browser.mjs',import.meta.url)),
  endpoint,secure,fileURLToPath(import.meta.url)],{encoding:'utf8',timeout:20000});
assert(!wrongWasm.error,String(wrongWasm.error));
assert.notEqual(wrongWasm.status,0,'wrong executed WASM was accepted');
assert.match(wrongWasm.stderr,/executed WASM differs from supplied build/);
console.log('wrong executed WASM rejected before output acceptance');
