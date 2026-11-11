// Installed startup acceptance owns an isolated context and stops on gameplay progress.
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { setTimeout as delay } from 'node:timers/promises';
import { connectCdp } from './capture-cdp.mjs';
import { observeBrowserBuild } from './browser-provenance.mjs';

const [endpoint, page, install, output, expectation = 'introduction'] = process.argv.slice(2);
assert(endpoint && page && install && output, 'usage: node dev/scripts/test-browser-startup.mjs CDP_URL PAGE_URL INSTALL_URL OUTPUT [introduction|autosave|menu]');
assert(['introduction', 'autosave', 'menu'].includes(expectation));
mkdirSync(output, { recursive: true });
const build = JSON.parse(readFileSync('target/web/build-provenance.json'));
const cdp = await connectCdp(endpoint);
const url = new URL(page);
url.search = new URLSearchParams({ install: new URL(install).href, capture: '1', ...(expectation === 'menu' ? { 'skip-startup': '1' } : {}) });
const initialUrl = new URL(url);
if (expectation === 'autosave') initialUrl.searchParams.set('scene', 'START');
const samples = [], shots = [];
let provenance, verifiedBuild, storageContracts, previous, moviePresented = false, failure, stage = 'fresh', requestId = 0;
const started = performance.now();
let deadline = started + 180000, movieTiming;
async function evaluate(expression) {
  const { result, exceptionDetails } = await cdp.call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  assert(!exceptionDetails, result.description ?? exceptionDetails?.text);
  return result.value;
}
async function autosave() {
  const request = async (type, fields = {}) => {
    const id = ++requestId;
    await evaluate(`window.__cyrusSessionRequest = ${JSON.stringify(JSON.stringify({type, id, ...fields}))}; true`);
    const deadline = performance.now() + 30000;
    while (performance.now() < deadline) {
      const result = await evaluate(`window.__cyrusSessionResult?.id === ${id} ? window.__cyrusSessionResult : null`);
      if (result) { assert(!result.error, result.error); return result; }
      await delay(100);
    }
    throw Error('autosave response deadline');
  };
  let result = await request('slots', { operation: { type: 'auto_save' } });
  const deadline = performance.now() + 30000;
  while (result.pending) {
    assert(performance.now() < deadline, 'autosave publication deadline');
    await delay(100); result = await request('result');
  }
  assert(result.stored, 'autosave was not stored');
}
async function shot(label) {
  const { data } = await cdp.call('Page.captureScreenshot', { format: 'png' });
  const path = `${label}.png`;
  writeFileSync(`${output}/${path}`, Buffer.from(data, 'base64'));
  shots.push(path);
}
try {
  provenance = await observeBrowserBuild(cdp, build, url);
  await cdp.call('Page.enable');
  await cdp.call('Emulation.setDeviceMetricsOverride', { width: 640, height: 480, deviceScaleFactor: 1, mobile: false });
  await cdp.call('Page.navigate', { url: initialUrl.href });
  let done = false;
  while (performance.now() < deadline) {
    const state = await evaluate(`(() => {
      const picker = document.getElementById('install-picker');
      if (picker && !picker.dataset.startupSubmitted) {
        picker.dataset.startupSubmitted = 'true';
        picker.requestSubmit();
      }
      window.__cyrusInspectScripts = true;
      return { startup: window.__cyrusStartup, menu: !window.__cyrusStartup || window.__cyrusStartup.state === "Menu" ? window.__cyrusMenu : null,
        world: window.__cyrusCapture, error: document.getElementById('failure')?.textContent,
        label: document.getElementById('label')?.textContent };
    })()`);
    samples.push({ stage, wall: (performance.now() - started) / 1000, ...state });
    assert(!state.error, state.error);
    assert(!state.label?.startsWith('Failed:'), state.label);
    const movie = state.menu?.movie;
    if (movie) assert(expectation === 'introduction', 'autosave or explicit menu skip dispatched a movie');
    const event = JSON.stringify([state.startup?.state, state.startup?.presentation?.title_visible, !!movie, movie?.loading, state.menu?.ready, state.world?.ready]);
    if (event !== previous) { await shot(`transition-${shots.length}`); previous = event; }
    if (movie?.frame > 0 && !movie.loading && !moviePresented) {
      assert.equal(movie.name, 'INTRO.SMK');
      assert(samples.some(sample => sample.startup?.presentation?.title_visible && !sample.menu?.movie), 'Introduction preceded the title');
      movieTiming = await evaluate(`window.cyrusGameAssets.read('movies/0.movie').then(bytes => {
        const {frames, frame_micros} = JSON.parse(new TextDecoder().decode(bytes));
        return {frames, frame_micros};
      })`);
      assert.equal(state.startup?.clock?.game_seconds, 0, 'game clock advanced before Introduction');
      await shot('introduction');
      moviePresented = true; done = true; break;
    }
    if (state.menu?.ready && !movie) {
      assert(expectation === 'menu' || stage === 'autosave', 'fresh browser startup opened the book instead of Introduction');
      await shot(stage === 'autosave' ? 'autosave-book' : 'book'); done = true; break;
    }
    if (state.world?.ready && state.world.scripts?.globals?.world_loop_frame > 0) {
      assert.equal(expectation, 'autosave');
      assert.equal(stage, 'fresh', 'valid autosave bypassed the book');
      await shot('world');
      await autosave();
      stage = 'autosave'; previous = undefined; deadline = performance.now() + 90000;
      await cdp.call('Page.navigate', { url: url.href });
    }
    await delay(100);
  }
  assert(done, 'startup endpoint deadline');
  const contracts = readFileSync('crates/cyrus-web/session-storage.js', 'utf8') + '\n' +
    readFileSync('dev/scripts/test-session-storage.mjs', 'utf8').replace(/^import .*;\n/, '');
  const contractUrl = 'data:text/javascript;base64,' + Buffer.from(contracts).toString('base64');
  storageContracts = await evaluate(`import(${JSON.stringify(contractUrl)}).then(async module => ({documents: await module.testSessionStorage(), slots: await module.testSessionSlots()}))`);
  assert(storageContracts.slots.passed);
  verifiedBuild = await provenance.verify();
  console.log(`${expectation}: passed; ${samples.length} samples, ${shots.length} screenshots`);
} catch (error) {
  failure = String(error.stack ?? error);
  throw error;
} finally {
  writeFileSync(`${output}/result.json`, JSON.stringify({ movieTiming, initialUrl: initialUrl.href, storageContracts, verifiedBuild, expectation, url: url.href, moviePresented, failure, samples, shots }, null, 2) + '\n');
  provenance?.stop();
  await cdp.close();
}
