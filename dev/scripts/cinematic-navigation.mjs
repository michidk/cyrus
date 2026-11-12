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
        audioContextTime:movie.audio?.context?.currentTime,
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

// Attach authored packet positions to the observed source, independently of its
// later native first-processing boundary. Never promote a schedule call to PCM.
export function observeMovieSchedule(original) {
  return function(block, ...args) {
    let probe, first;
    try { probe = globalThis.__sourceGeneration; first = probe?.sources.length; }
    catch (error) { globalThis.__cinematicClockError = String(error); }
    const result = original.call(this, block, ...args);
    try {
      if (probe && first !== undefined) for (const source of probe.sources.slice(first)) {
        source.movieStart = block.start + source.offset;
        source.videoEnd = this.videoEnd;
        source.movieEnd = source.duration === null || source.duration === undefined
          ? block.end : source.movieStart + source.duration;
      }
    } catch (error) { globalThis.__cinematicClockError = String(error); }
    return result;
  };
}

// The returned video clock must not hide an audio cursor beyond decoded video.
// Compare integer output-sample positions rather than fitting a wall-time delay.
export function requireMovieHorizon(reads, rate) {
  for (const read of reads) if (read.packetPosition !== undefined) {
    if (Math.round(read.packetPosition * rate) > Math.round(read.videoEnd * rate)) {
      throw Error('movie audio consumed beyond decoded video');
    }
  }
}
