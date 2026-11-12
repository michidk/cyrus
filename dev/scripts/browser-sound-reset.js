// Evaluate after session-sample-graph.js in an installed capture-enabled browser.
// The host drives the real book/menu input between these bounded PCM observations.
(() => {
  const results = {};
  async function sample(name, present) {
    const graph = globalThis.__sessionSamples;
    if (!graph) throw Error('install the production sample graph tap first');
    const state = graph.status();
    if (state.error || !state.ready) throw Error(state.error ?? 'sample graph is not ready');
    // Already mixed CPAL buffers may drain after stop. Wait past the last
    // submitted buffer at this boundary, then examine actual graph quanta.
    const last = Math.max(state.now, ...state.buffers.map(b => b.frame + b.length));
    const deadline = performance.now() + 15000;
    let boundary = last;
    if (present) {
      // A slow rendered host may submit separated buffers. Observe a new
      // nonzero submission rather than requiring unrelated host gaplessness.
      let buffer;
      while (!(buffer = graph.status().buffers.find(b => b.frame >= last && b.energy > .001))) {
        if (performance.now() >= deadline) throw Error('bounded active submission expired');
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      boundary = buffer.frame;
    }
    const start = Math.ceil(boundary / 128) * 128 + 128, end = start + 8192;
    while (graph.status().last < end) {
      if (performance.now() >= deadline) throw Error('bounded sample readback expired');
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    const blocks = graph.blocks.filter(b => b.frame >= start && b.frame < end);
    if (blocks.length !== 64 || blocks.some((b, i) => b.frame !== start + i * 128)) {
      throw Error('missing or duplicate output quantum');
    }
    const energy = blocks.reduce((sum, b) => sum + [...b.left, ...b.right].reduce((s, x) => s + x*x, 0), 0);
    if (present ? energy <= .001 : energy > 1e-12) throw Error(`sample cancellation mismatch: ${name}, energy=${energy}`);
    return results[name] = {start, end, frames: end-start, energy, present};
  }
  globalThis.__cyrusSoundReset = {sample, results};
})();
