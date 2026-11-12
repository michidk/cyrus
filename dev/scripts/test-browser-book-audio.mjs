// Installed public Options-to-Main PCM acceptance; no submitted-buffer substitution.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {existsSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {connectCdp} from './capture-cdp.mjs';
import {observeBrowserBuild} from './browser-provenance.mjs';

const [endpoint, page, install, reference, output] = process.argv.slice(2);
assert(endpoint && page && install && reference && output,
  'usage: node dev/scripts/test-browser-book-audio.mjs CDP_URL PAGE_URL INSTALL_URL EFFECT_WAV OUTPUT');
assert(!existsSync(output), 'output exists; preserve evidence and choose a new directory');
mkdirSync(output, {recursive: true});
const cdp = await connectCdp(endpoint);
const url = new URL(page);
url.search = new URLSearchParams({install: new URL(install).href, capture: '1', 'skip-startup': '1'});
let provenance;
async function evaluate(expression) {
  const {result, exceptionDetails} = await cdp.call('Runtime.evaluate', {
    expression, returnByValue: true, awaitPromise: true,
  });
  assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
  return result.value;
}
async function until(label, condition, limit = 30000) {
  const deadline = performance.now() + limit;
  let clock, progressedAt = performance.now();
  while (performance.now() < deadline) {
    const value = await condition();
    if (value) return value;
    const state = await evaluate('({clock:window.__cyrusStartup?.clock?.game_seconds,error:document.getElementById("failure")?.textContent,probe:window.__sessionSamples?.error})');
    assert(!state.error, state.error);
    assert(!state.probe, state.probe);
    if (state.clock !== clock) {clock = state.clock; progressedAt = performance.now();}
    if (clock !== undefined) assert(performance.now() - progressedAt < 20000, `${label}: game clock stopped`);
    await delay(25);
  }
  throw Error(`${label}: bounded acceptance expired`);
}
async function input(type, key, value) {
  await cdp.call('Input.dispatchKeyEvent', {type, key, code: key, windowsVirtualKeyCode: value});
}
async function enter() {
  await input('keyDown', 'Enter', 13);
  await delay(100);
  await input('keyUp', 'Enter', 13);
}
async function select(action) {
  for (let attempt = 0; attempt < 10; attempt++) {
    const menu = await evaluate('window.__cyrusMenu');
    assert(menu.ready, 'selection requires a presented ready page');
    if (menu.entries[menu.selected]?.action === action) return;
    await input('keyDown', 'ArrowDown', 40);
    try {
      await until('selection acknowledgement', () => evaluate(`window.__cyrusMenu.selected !== ${menu.selected}`), 5000);
    } finally {await input('keyUp', 'ArrowDown', 40);}
    await until('selection presentation', () => evaluate('window.__cyrusMenu.ready'));
  }
  throw Error(`cannot select installed action ${action}`);
}
try {
  const build = JSON.parse(readFileSync(process.env.CYRUS_BROWSER_BUILD_RECORD ?? 'target/web/build-provenance.json'));
  provenance = await observeBrowserBuild(cdp, build, url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride', {width: 640, height: 480, deviceScaleFactor: 1, mobile: false});
  await cdp.call('Page.addScriptToEvaluateOnNewDocument', {
    source: readFileSync(new URL('./session-sample-graph.js', import.meta.url), 'utf8'),
  });
  await until('install page navigation', async () => {
    const navigation = await cdp.call('Page.navigate', {url: url.href});
    return !navigation.errorText;
  });
  await until('initial book readiness', () => evaluate(`(() => {
    const picker = document.getElementById('install-picker');
    if (picker && !picker.dataset.submitted) {picker.dataset.submitted = '1'; picker.requestSubmit();}
    return window.__cyrusMenu?.page === 0 && window.__cyrusMenu.ready;
  })()`), 180000);
  await select(4);
  await enter();
  await until('Options readiness', () => evaluate('window.__cyrusMenu.page === 4 && window.__cyrusMenu.ready'));
  await select(400);
  await until('sample graph readiness', () => evaluate('window.__sessionSamples.status().ready'));
  await evaluate(`(() => {
    window.__bookAudio = {};
    __sessionSamples.blocks = []; __sessionSamples.buffers = [];
    addEventListener('keydown', e => {
      if (e.key === 'Enter' && !__bookAudio.input)
        __bookAudio.input = {wall: performance.now(), frame: __sessionSamples.status().now, menu: structuredClone(__cyrusMenu)};
    }, {capture: true});
    __bookAudio.timer = setInterval(() => {
      if (__bookAudio.input && __cyrusMenu.page === 0 && __cyrusMenu.ready && !__bookAudio.ack)
        __bookAudio.ack = {wall: performance.now(), frame: __sessionSamples.status().now, menu: structuredClone(__cyrusMenu)};
    }, 10);
  })()`);
  await enter();
  await until('Main acknowledgement', () => evaluate('window.__bookAudio.ack'));
  await delay(1200);
  const record = await evaluate(`(() => {
    clearInterval(__bookAudio.timer);
    const p = __sessionSamples, start = Math.floor(__bookAudio.input.frame / 128) * 128, end = p.status().last;
    if (p.error) throw Error(p.error);
    const blocks = p.blocks.filter(b => b.frame >= start && b.frame < end);
    let next = start;
    for (const b of blocks) {if (b.frame !== next) throw Error('missing or duplicate graph quantum'); next += 128;}
    if (next !== end) throw Error('incomplete graph window');
    window.__bookAudioPcm = new Int16Array((end - start) * 2);
    for (const b of blocks) for (let i = 0; i < 128; i++) for (let ch = 0; ch < 2; ch++)
      __bookAudioPcm[(b.frame - start + i) * 2 + ch] = Math.max(-32768, Math.min(32767, Math.round(b[ch ? 'right' : 'left'][i] * 32768)));
    return {start, end, frames: end - start, rate: p.status().rate, transition: __bookAudio, buffers: p.status().buffers};
  })()`);
  writeFileSync(`${output}/capture.json`, JSON.stringify(record, null, 2));
  const parts = [];
  for (let offset = 0; offset < record.frames * 4; offset += 32768) {
    const length = Math.min(32768, record.frames * 4 - offset);
    parts.push(Buffer.from(await evaluate(`btoa(String.fromCharCode(...new Uint8Array(__bookAudioPcm.buffer, ${offset}, ${length})))`), 'base64'));
  }
  const pcm = Buffer.concat(parts), header = Buffer.alloc(44);
  header.write('RIFF'); header.writeUInt32LE(36 + pcm.length, 4); header.write('WAVEfmt ', 8);
  header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(2, 22);
  header.writeUInt32LE(record.rate, 24); header.writeUInt32LE(record.rate * 4, 28);
  header.writeUInt16LE(4, 32); header.writeUInt16LE(16, 34); header.write('data', 36); header.writeUInt32LE(pcm.length, 40);
  writeFileSync(`${output}/audio.wav`, Buffer.concat([header, pcm]));
  writeFileSync(`${output}/provenance.json`, JSON.stringify(await provenance.verify(), null, 2));
  const screenshot = await cdp.call('Page.captureScreenshot', {format: 'png'});
  writeFileSync(`${output}/main.png`, Buffer.from(screenshot.data, 'base64'));
  execFileSync(process.env.CYRUS_ANALYSIS_PYTHON ?? 'python3', [
    new URL('./book-audio-continuity.py', import.meta.url).pathname,
    `${output}/audio.wav`, `${output}/continuity.json`, '--reference', reference,
  ], {stdio: 'inherit'});
  console.log(JSON.stringify({frames: record.frames, rate: record.rate, inputToReady: (record.transition.ack.wall - record.transition.input.wall) / 1000}));
} catch (error) {
  writeFileSync(`${output}/failure.json`, JSON.stringify({error: String(error), events: cdp.events}, null, 2));
  throw error;
} finally {provenance?.stop(); await cdp.close();}
