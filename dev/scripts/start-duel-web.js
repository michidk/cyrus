// Plays the START pirate duel in the browser build with installed keys only (DOM key events).
// Load `?scene=START&capture=1&install=<server>/original/`, connect to the server, wait for the
// ship deck, then evaluate this file in the page (preview_evaluate or the devtools console).
// `capture=1` publishes window.__cyrusCapture; setting __cyrusInspectScripts adds the first 64
// script actors (the native `scripts` command's data). Progress lands in window.__duelLog and the
// outcome in window.__duelDone ('both defeated' | 'player at 0' | 'step limit' | 'aborted').
// Holds are long because a software-rendered tab runs at about 2 frames per second; a key must
// span a frame to be sampled. Set window.__duelAbort = true to stop. The story then talks
// (Brennan) and loads world 1 by itself.
(() => {
  window.__cyrusInspectScripts = true;
  if (!window.__cyrusCapture || !window.__cyrusCapture.scripts) return 'not ready';
  const canvas = document.querySelector('canvas');
  canvas.tabIndex = 0;
  canvas.focus();
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const send = (type, code) =>
    canvas.dispatchEvent(new KeyboardEvent(type, { code, key: code, bubbles: true, cancelable: true }));
  const hold = async (code, ms) => {
    send('keydown', code);
    await sleep(ms);
    send('keyup', code);
    await sleep(700);
  };
  const actors = () => Object.fromEntries(window.__cyrusCapture.scripts.actors.map((a) => [a.name, a]));
  const wrap = (v, half) => (((v + half) % (2 * half)) + 2 * half) % (2 * half) - half;
  const bearing = (me, other) => {
    const p = (a) => a.spatial.effective.position;
    const dx = (p(other)[0] - p(me)[0]) / 256;
    const dz = (p(other)[2] - p(me)[2]) / 256;
    return [(((-Math.atan2(dx, dz) * 2048) / (2 * Math.PI)) % 2048 + 2048) % 2048, Math.hypot(dx, dz)];
  };
  window.__duelLog = [];
  window.__duelDone = false;
  window.__duelAbort = false;
  (async () => {
    await hold('KeyS', 1500); // quick sword
    let turnsLeftForPositive = null;
    for (let step = 0; step < 600 && !window.__duelAbort; step++) {
      const found = actors();
      const me = found.CYRUS;
      const pirates = ['SPIRATE1', 'SPIRATE2'].map((n) => found[n]).filter(Boolean);
      const alive = pirates.filter((p) => p.attributes[3] > 0 && !(p.control && p.control.killed));
      if (step % 3 === 0) window.__duelLog.push([step, pirates.map((p) => [p.name, p.attributes[3]]), me.attributes[3]]);
      if (!alive.length) return void (window.__duelDone = 'both defeated');
      if (me.attributes[3] <= 0) return void (window.__duelDone = 'player at 0');
      const target = alive.reduce((a, b) => (bearing(me, a)[1] <= bearing(me, b)[1] ? a : b));
      const [want, distance] = bearing(me, target);
      const current = me.spatial.effective.angles[1];
      const diff = wrap(want - current, 1024);
      if (Math.abs(diff) > 70) {
        const key = diff > 0 === (turnsLeftForPositive !== false) ? 'ArrowLeft' : 'ArrowRight';
        await hold(key, 500);
        if (turnsLeftForPositive === null) {
          const moved = wrap(actors().CYRUS.spatial.effective.angles[1] - current, 1024);
          if (Math.abs(moved) > 5) turnsLeftForPositive = moved * diff > 0;
        }
      } else if (distance > 120) await hold('ArrowUp', 900);
      else await hold('ControlLeft', 500);
    }
    window.__duelDone = window.__duelAbort ? 'aborted' : 'step limit';
  })();
  return 'started';
})();
