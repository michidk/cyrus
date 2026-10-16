// Prepare game assets in the shared WASM worker from read-only local files or HTTP.
export async function openGameAssets(source, { createWorker = () => new Worker('./asset-worker.js', { type: 'module' }), progress = () => {} } = {}) {
  const worker = createWorker();
  const files = new Map(), pending = new Map(), requests = new Map();
  const dialoguePaths = new Map();
  let nextId = 1, worldPaths = [], failure;
  worker.onmessage = ({ data }) => {
    if (data.kind === 'progress') { progress(data.done, data.total, data.name); return; }
    const request = requests.get(data.id);
    if (!request) return;
    requests.delete(data.id);
    if (data.kind === 'error') request.reject(new Error(data.message));
    else request.resolve(data);
  };
  worker.onerror = event => {
    failure = new Error(event.message || 'Asset worker failed');
    for (const request of requests.values()) request.reject(failure);
    requests.clear();
  };
  function request(kind, data = {}) {
    if (failure) return Promise.reject(failure);
    const id = nextId++;
    return new Promise((resolve, reject) => {
      requests.set(id, { resolve, reject });
      try { worker.postMessage({ kind, id, ...data }); }
      catch (error) { requests.delete(id); reject(error); }
    });
  }
  function group(path) {
    const root = path.split('/')[0];
    if (['startup', 'menu', 'hud', 'sounds', 'cheats', 'console'].includes(root)) return root;
    if (root === 'music' || root === 'movies') return path;
    if (root === 'dialogue') return path.replace(/\.(dialogue|wav)$/, '');
    return root.replace(/\.(glb|animations|scripts|environment|player|player-models)$/, '');
  }
  async function read(path) {
    if (files.has(path)) return files.get(path);
    const key = group(path);
    if (!pending.has(key)) {
      const task = request('game-prepare', { path }).then(result => {
        const scene = !path.includes('/') || path.includes('.player-models/');
        if (scene) {
          for (const old of worldPaths) files.delete(old);
          worldPaths = result.files.map(([name]) => name);
        }
        for (const [name, bytes] of result.files) files.set(name, bytes);
        if (path.startsWith('dialogue/')) {
          dialoguePaths.delete(key);
          dialoguePaths.set(key, result.files.map(([name]) => name));
          while (dialoguePaths.size > 20) {
            const oldest = dialoguePaths.keys().next().value;
            for (const name of dialoguePaths.get(oldest)) files.delete(name);
            dialoguePaths.delete(oldest);
          }
        }
        return new Map(result.files);
      }).finally(() => pending.delete(key));
      pending.set(key, task);
    }
    const prepared = await pending.get(key);
    const bytes = files.get(path) ?? prepared.get(path);
    if (!bytes) throw new Error('Game preparation did not produce ' + path);
    return bytes;
  }
  function movieIndex(url) {
    const match = /(?:^|\/)movies\/(\d+)\.(?:smk|wav)$/.exec(url);
    if (!match) throw new Error('Invalid movie path ' + url);
    return Number(match[1]);
  }
  function response(url, { signal } = {}) {
    const index = movieIndex(url);
    let start = 0;
    return new Response(new ReadableStream({
      async pull(controller) {
        try {
          if (signal?.aborted) throw signal.reason || new Error('Movie cancelled');
          const result = await request('movie-range', { index, start, length: 256 * 1024 });
          if (signal?.aborted) throw signal.reason || new Error('Movie cancelled');
          start += result.bytes.length;
          if (result.bytes.length) controller.enqueue(result.bytes);
          if (start >= result.total) controller.close();
        } catch (error) { controller.error(error); }
      },
    }), { headers: { 'Content-Type': 'application/octet-stream' } });
  }
  function close() {
    failure = new Error('Asset worker closed');
    worker.terminate();
    for (const request of requests.values()) request.reject(new Error('Asset worker closed'));
    requests.clear(); files.clear(); dialoguePaths.clear();
  }
  try {
    const opened = await request('game-open', { source });
    return { read, response, worlds: JSON.parse(opened.worlds), close };
  } catch (error) {
    close();
    throw error;
  }
}
