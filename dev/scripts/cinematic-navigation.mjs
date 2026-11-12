import assert from 'node:assert/strict';
import {setTimeout as wait} from 'node:timers/promises';

// Runtime recovery starts HTTP and Chrome independently. A successful CDP
// command can still describe a failed network navigation; inspect errorText.
export async function navigateCinematic(cdp, page, pause = () => wait(250), attempts = 80) {
  let error;
  for (let i = 0; i < attempts; i++) {
    const result = await cdp.call('Page.navigate', {url:page});
    if (!result.errorText) return result;
    error = result.errorText;
    await pause();
  }
  assert.fail(`cinematic HTTP navigation unavailable: ${error}`);
}

// Freeze application animation after capturing the generation endpoint. Trace
// draining and PCM readback must not keep decoding/rendering the entire movie.
export function installCinematicEndpoint() {
  const schedule = globalThis.requestAnimationFrame;
  globalThis.requestAnimationFrame = callback => schedule.call(globalThis, time => {
    if (!globalThis.__cinematicEndpointStopped) callback.call(globalThis, time);
  });
}

// Wrap the module's real exported movieTime binding once, at movie creation.
// Observe its returned clock, not a guessed clamp of a nested audio getter.
export function observeMovieTime(original, movieForId, records) {
  return function(...args) {
    const index = records.length;
    try { console.timeStamp('movie-time-begin:' + index); }
    catch (error) { globalThis.__cinematicClockError = String(error); }
    const value = original.apply(this, args);
    try {
      const movie = movieForId(args[0]);
      records.push({value, id:args[0], videoEnd:movie.videoEnd,
        position:movie.position, origin:movie.origin,
        packetPosition:movie.audio?.position, packetOrigin:movie.audio?.origin,
        paused:movie.audio?.paused});
      console.timeStamp('movie-time-end:' + index);
    } catch (error) { globalThis.__cinematicClockError = String(error); }
    return value;
  };
}

// This function is injected as parsed JavaScript, avoiding separately edited
// string fragments whose local declarations can collide after a rig change.
export function cinematicProgress(minimumFrame) {
  const picker = document.getElementById('install-picker');
  if (picker && !picker.dataset.sent) {picker.dataset.sent = 1;picker.requestSubmit();}
  const movie = globalThis.__cyrusMenu?.movie;
  const probe = globalThis.__sessionSamples;
  const signalEnergy = movie?.frame >= minimumFrame && probe
    ? probe.blocks.slice(-200).reduce((sum, block) => sum
      + block.left.reduce((s, value) => s + value * value, 0)
      + block.right.reduce((s, value) => s + value * value, 0), 0) : 0;
  return {movie, signalEnergy, error:document.getElementById('failure')?.textContent,
    observer:globalThis.__sourceGeneration?.error || globalThis.__cinematicClockError,
    graph:probe?.error};
}
