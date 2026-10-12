import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const handlers = new Map();
let finishWrite;
const writing = new Promise(resolve => { finishWrite = resolve; });
let stored;
vm.runInNewContext(await readFile(new URL("../../crates/cyrus-web/web/sw.js", import.meta.url), "utf8"), {
  URL, console,
  self: { location: { origin: "https://game.test" }, addEventListener: (name, fn) => handlers.set(name, fn) },
  caches: { open: async () => ({ match: async () => undefined, put: async (_, response) => {
    stored = await response.text();
    await writing;
  } }) },
  fetch: async () => new Response("scene bytes", { headers: { "content-length": "11" } }),
});
let response, lifetime;
handlers.get("fetch")({
  request: new Request("https://game.test/scene.glb"),
  respondWith: promise => { response = promise; },
  waitUntil: promise => { lifetime = promise; },
});
// Cache writes may stall or exceed quota; the game must still receive its response.
let timer;
try {
  const received = await Promise.race([
    response.then(r => r.text()),
    new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("game waits for cache write")), 200); }),
  ]);
  assert.equal(received, "scene bytes");
  assert.ok(lifetime);
  finishWrite();
  await lifetime;
  assert.equal(stored, "scene bytes");
} finally {
  clearTimeout(timer);
  finishWrite();
}
