// Execute installed replacement ownership controls and bind the receipt to loaded WASM.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
const [endpoint, page, wasm, output] = process.argv.slice(2);
assert(endpoint && page && wasm && output, 'usage: CDP PAGE WASM OUTPUT');
const expected = createHash('sha256').update(readFileSync(wasm)).digest('hex');
const cdp = await connectCdp(endpoint), pending = [], executed = [], errors = [];
try {
  cdp.onEvent(({method, params}) => {
    if (method === 'Runtime.exceptionThrown') errors.push(params);
    if (method === 'Debugger.scriptParsed' && params.scriptLanguage === 'WebAssembly') {
      pending.push(cdp.call('Debugger.getScriptSource', {scriptId: params.scriptId}).then(source => {
        assert(source.bytecode, 'executed WASM bytecode unavailable');
        const hash = createHash('sha256').update(Buffer.from(source.bytecode, 'base64')).digest('hex');
        assert.equal(hash, expected, 'different WASM executed');
        executed.push(hash);
      }).catch(error => errors.push(String(error))));
    }
  });
  await cdp.call('Runtime.enable');
  await cdp.call('Debugger.enable');
  await cdp.call('Page.navigate', {url: page});
  let report;
  const deadline = Date.now() + 30000;
  while (!report) {
    assert(Date.now() < deadline, 'bounded replacement lifecycle run expired');
    const response = await cdp.call('Runtime.evaluate', {
      expression: 'globalThis.replacementResult', returnByValue: true,
    });
    assert(!response.exceptionDetails, 'replacement report evaluation failed');
    report = response.result.value;
    if (!report) await delay(100);
  }
  await Promise.all(pending);
  assert.deepEqual(errors, []);
  assert(executed.includes(expected), 'no independently observed WASM identity');
  const rows = report.replacement_lifecycle;
  assert.equal(rows.length, 8);
  assert.deepEqual(rows.map(row => [row.branch, row.rejected]),
    ['Forced', 'Ordinary', 'SkippedFrames', 'Default'].flatMap(branch => [[branch, false], [branch, true]]));
  for (const row of rows) {
    assert.equal(row.plays, 1);
    assert.equal(row.replayed, false);
    assert.equal(row.snapshot_failure_retained, row.rejected);
    assert.equal(row.released, row.rejected ? 64 : 2);
  }
  writeFileSync(output, JSON.stringify({executed_wasm_sha256: expected, ...report}, null, 2) + '\n');
} finally {
  await cdp.close();
}
