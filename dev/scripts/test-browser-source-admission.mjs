// A source can render before native start returns; the tap must already be wired.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint, page, output, observerPath] = process.argv.slice(2);
assert(endpoint && page && output, 'usage: CDP_URL LOOPBACK_PAGE RESULT_JSON [OBSERVER_SCRIPT]');
const graph = readFileSync(observerPath ?? new URL('./session-sample-graph.js', import.meta.url), 'utf8');
const cdp = await connectCdp(endpoint);
try {
  await cdp.call('Page.enable');
  let navigated = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    if (!(await cdp.call('Page.navigate', {url: page})).errorText) { navigated = true; break; }
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  assert(navigated, 'source admission control server unavailable');
  let ready = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    const state = await cdp.call('Runtime.evaluate', {returnByValue: true,
      expression: `location.href === ${JSON.stringify(page)} && document.readyState === 'complete'`});
    if (state.result.value) { ready = true; break; }
    await new Promise(resolve => setTimeout(resolve, 25));
  }
  assert(ready, 'source admission control page unavailable');
  const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {
    awaitPromise: true, returnByValue: true, userGesture: true,
    expression: `(async () => {
      const native = AudioBufferSourceNode.prototype.start;
      AudioBufferSourceNode.prototype.start = function(...args) {
        const before = this.context.currentTime;
        const result = native.apply(this, args);
        this.admission = {before, after: this.context.currentTime};
        if (this.holdReturn) {
          const until = performance.now() + 80;
          while (performance.now() < until) {}
        }
        return result;
      };
      ${graph}
      const context = new AudioContext({sampleRate: 48000});
      const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
      try {
        await context.resume();
        const bootstrap = context.createBufferSource();
        bootstrap.buffer = context.createBuffer(2, 128, context.sampleRate);
        bootstrap.connect(context.destination); bootstrap.start();
        for (let i = 0; i < 200 && !__sessionSamples.status().ready; i++) await wait(10);
        await wait(150);
        const source = context.createBufferSource();
        source.buffer = context.createBuffer(2, 8192, context.sampleRate);
        for (let channel = 0; channel < 2; channel++) {
          const data = source.buffer.getChannelData(channel);
          for (let i = 0; i < data.length; i++) data[i] = (channel + 1) * (1 + i % 31) / 256;
        }
        source.connect(context.destination);
        const start = Math.ceil((context.currentTime + 0.02) * context.sampleRate / 128) * 128;
        source.holdReturn = true; source.start(start / context.sampleRate);
        const returned = context.currentTime, end = start + source.buffer.length;
        for (let i = 0; i < 200 && (__sessionSamples.blocks.at(-1)?.frame ?? 0) < end; i++) await wait(10);
        const metadata = __sessionSamples.capture(start, end);
        let error = 0, energy = 0, first;
        for (let i = 0; i < source.buffer.length; i++) for (let channel = 0; channel < 2; channel++) {
          const actual = __sessionSamples.capturedPcm[i * 2 + channel];
          const expected = source.buffer.getChannelData(channel)[i];
          if (actual !== 0 && first === undefined) first = start + i;
          error += (actual - expected) ** 2; energy += expected ** 2;
        }
        return {start, end, first, rate: context.sampleRate, returned, admission: source.admission,
          error, energy, ratio: error / energy, graphError: metadata.error};
      } finally { await context.close(); }
    })()`,
  });
  assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
  const receipt = result.value;
  writeFileSync(output, JSON.stringify(receipt, null, 2) + '\n');
  assert(receipt.admission.after < receipt.start / receipt.rate, 'control must admit a future source');
  assert(receipt.returned > (receipt.start + 128) / receipt.rate, 'control must render before start returns');
  assert.equal(receipt.graphError, null);
  assert.equal(receipt.first, receipt.start, 'observer missed source onset during native return');
  assert(receipt.ratio < 1e-6, 'source consumption differs from admitted future sample boundary');
  console.log(JSON.stringify(receipt));
} finally { await cdp.close(); }
