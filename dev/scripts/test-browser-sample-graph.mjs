// Actual WebAudio output and observer-failure isolation, without installed assets.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';

const [endpoint, page] = process.argv.slice(2);
assert(endpoint && page, 'usage: node dev/scripts/test-browser-sample-graph.mjs CDP_URL SECURE_OR_LOOPBACK_PAGE');
const cdp = await connectCdp(endpoint);
async function evaluate(expression) {
  const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {
    expression, returnByValue: true, awaitPromise: true,
  });
  assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
  return result.value;
}
async function until(expression) {
  const deadline = performance.now() + 10000;
  while (performance.now() < deadline) {
    const value = await evaluate(expression);
    if (value) return value;
    await delay(25);
  }
  throw Error(`audio control deadline: ${expression}; ${JSON.stringify(await evaluate('window.__sessionSamples?.status()'))}`);
}
try {
  await cdp.call('Page.enable');
  let navigated = false;
  for (let attempt = 0; attempt < 40; attempt++) {
    if (!(await cdp.call('Page.navigate', {url: page})).errorText) { navigated = true; break; }
    await delay(250);
  }
  assert(navigated, 'sample graph control page unavailable');
  await until(`location.href === ${JSON.stringify(page)} && document.readyState === 'complete'`);
  // Isolate the observer from any application scripts on the supplied local page.
  await evaluate("document.open(); document.write('<title>Sample graph controls</title>'); document.close(); true");
  await evaluate(readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8'));
  await cdp.call('Input.dispatchMouseEvent', {type: 'mousePressed', x: 2, y: 2, button: 'left', clickCount: 1});
  await cdp.call('Input.dispatchMouseEvent', {type: 'mouseReleased', x: 2, y: 2, button: 'left', clickCount: 1});
  await evaluate(`(() => {
    window.controlContext = new AudioContext({sampleRate: 48000});
    window.controlSource = buffer => {
      const source = controlContext.createBufferSource();
      source.buffer = buffer; source.connect(controlContext.destination); return source;
    };
    const silent = controlContext.createBuffer(2, 128, controlContext.sampleRate);
    controlSource(silent).start(); return controlContext.resume();
  })()`);
  await until('__sessionSamples.status().ready');
  await evaluate(`(() => {
    const buffer = controlContext.createBuffer(2, 8192, controlContext.sampleRate);
    for (let ch = 0; ch < 2; ch++) for (let i = 0; i < buffer.length; i++)
      buffer.getChannelData(ch)[i] = Math.sin(i * 2 * Math.PI * 440 / controlContext.sampleRate) * 0.1;
    const start = controlContext.currentTime + 0.1;
    window.controlStart = Math.ceil(start * controlContext.sampleRate / 128) * 128;
    window.controlEnd = Math.floor((start * controlContext.sampleRate + buffer.length) / 128) * 128;
    controlSource(buffer).start(start); return true;
  })()`);
  await until('__sessionSamples.status().last >= controlEnd');
  const rendered = await evaluate('__sessionSamples.match(controlStart, controlEnd)');
  assert(rendered.frames >= 7936);
  assert.equal(await evaluate('__sessionSamples.error'), null);
  // Hide only the observer API. The real context and source still render normally.
  await evaluate(`(() => {
    Object.defineProperty(controlContext, 'audioWorklet', {value: undefined});
    const source = controlSource(controlContext.createBuffer(2, 1024, controlContext.sampleRate));
    source.onended = () => {window.rejectedObserverSourceEnded = true;};
    source.start(); return true;
  })()`);
  await until('window.rejectedObserverSourceEnded');
  assert.match(await evaluate('__sessionSamples.error'), /AudioWorklet/);
  assert.equal(await evaluate('__sessionSamples.status().ready'), false);
  console.log(JSON.stringify({rendered, rejectedObserverSourceEnded: true}));
} finally {
  await evaluate('window.controlContext?.close()').catch(() => {});
  await cdp.close();
}
