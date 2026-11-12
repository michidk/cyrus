// Export one immutable graph window; persist the exact rejected timeline too.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync, writeFileSync} from 'node:fs';

export async function captureSampleGraph(cdp, output, start, end) {
  assert(Number.isSafeInteger(start) && Number.isSafeInteger(end), 'integer sample boundaries required');
  return capture(cdp, output, `({start: ${start}, end: ${end}})`, 'null');
}

// Observe the endpoint and freeze its delivered PCM in one browser task, before
// screenshots or asynchronous readback can extend the window or source timeline.
export async function captureSampleGraphAtEndpoint(cdp, output, observation = 'null') {
  return capture(cdp, output, `({start: __sessionSamples.blocks[0]?.frame,
    end: __sessionSamples.blocks.at(-1)?.frame + 128})`, observation);
}

async function capture(cdp, output, window, observation) {
  assert(!existsSync(output + '.json') && !existsSync(output + '.f32le'), 'capture output already exists');
  async function evaluate(expression) {
    const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {expression, returnByValue: true});
    assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
    return result.value;
  }
  const metadata = await evaluate(`(() => {
    try {
      const observation = structuredClone((${observation}));
      const {start, end} = ${window};
      const metadata = __sessionSamples.capture(start, end);
      return {...metadata, observation};
    }
    catch (error) { return {...__sessionSamples.lastCapture, error: String(error)}; }
  })()`);
  metadata.exported = false;
  writeFileSync(output + '.json', JSON.stringify(metadata, null, 2) + '\n', {flag: 'wx'});
  assert(!metadata.error, metadata.error);
  const {start, end} = metadata;
  const parts = [];
  for (let at = 0; at < (end - start) * 8; at += 32768) {
    const length = Math.min(32768, (end - start) * 8 - at);
    parts.push(Buffer.from(await evaluate(`btoa(String.fromCharCode(...new Uint8Array(__sessionSamples.capturedPcm.buffer, ${at}, ${length})))`), 'base64'));
  }
  const pcm = Buffer.concat(parts);
  assert.equal(pcm.length, (end - start) * 8, 'incomplete PCM readback');
  writeFileSync(output + '.f32le', pcm, {flag: 'wx'});
  metadata.exported = true;
  metadata.pcm = {format: 'f32le', channels: 2, bytes: pcm.length, sha256: createHash('sha256').update(pcm).digest('hex')};
  writeFileSync(output + '.json', JSON.stringify(metadata, null, 2) + '\n');
  return metadata;
}
