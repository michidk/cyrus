// Shared-cache contract without browser or game-data fixtures.
const assert = require("node:assert/strict");
const vm = require("node:vm");
const fs = require("node:fs");
const events = {};
const saved = new Map();
const calls = [];
let offline = false;
let ignoresRange = false;
let finishWrite;
let delayWrite = false;
let brokenBodies = 0;
const base = "https://example.test/tools/";
const context = {
  URL, console, Request, Response, Headers,
  self: { location: new URL(base + "sw.js"), addEventListener: (name, fn) => events[name] = fn },
  caches: { open: async () => ({ match: async req => saved.get(req.url), put: async (req, value) => { if (delayWrite) await new Promise(resolve => finishWrite = resolve); saved.set(req.url, value); } }) },
  fetch: async (request, options) => {
    calls.push([typeof request === "string" ? request : request.url, options]);
    if (offline) throw new Error("offline");
    if (brokenBodies > 0) {
      brokenBodies--;
      return new Response(new ReadableStream({ start(controller) {
        controller.error(new TypeError("connection reset during response body"));
      } }));
    }
    if ((typeof request === "string" ? request : request.url).includes("range=")) {
      if (ignoresRange) return new Response("whole file");
      return new Response("part", { status: 206, headers: { "content-range": "bytes 0-3/8" } });
    }
    return new Response("preview", { headers: { "last-modified": "today", "content-length": "7" } });
  },
};
vm.runInNewContext(fs.readFileSync(__dirname + "/sw.js", "utf8"), context);
function request(path, origin = base, drain = true, headers = {}) {
  let response;
  const background = [];
  events.fetch({ request: new Request(origin + path, { headers }), respondWith: promise => response = promise, waitUntil: promise => background.push(promise) });
  return response?.then(async value => { if (drain) await Promise.all(background); return value; });
}
(async () => {
  assert.equal(request("cyrus-config/cache/movies/0.smk"), undefined, "video must stream without a cache clone downloading it all");
  assert.equal(request("cyrus-config/cache/movies/0.wav"), undefined, "audio must stream through the browser media cache");
  assert.equal(request("cyrus-config/cache/sound.wav", base, true, { Range: "bytes=0-3" }), undefined, "partial requests must bypass full-response caching");
  const immutable = "cyrus-config/cache/viewer/" + "a".repeat(64) + "/model.glb";
  await request(immutable);
  assert.equal(calls.length, 1);
  await request(immutable);
  assert.equal(calls.length, 1, "generation media should reuse its cached response");
  await request("cyrus-config/cache/viewer/catalog.json");
  await request("cyrus-config/cache/viewer/catalog.json");
  assert.equal(calls.at(-1)[1].method, "HEAD", "current catalog must revalidate");
  const source = "original/fonts/FONTSEL.FNT?v=" + "b".repeat(64);
  await request(source);
  const downloaded = calls.length;
  await request(source);
  assert.equal(calls.length, downloaded, "original bytes reuse their digest cache");
  const chunk = source + "&range=0-3";
  await request(chunk);
  const cachedChunk = await request(chunk);
  assert.equal(cachedChunk.status, 200);
  assert.equal(cachedChunk.headers.get("content-range"), "bytes 0-3/8");
  assert.equal(await cachedChunk.text(), "part");
  assert.equal(request("original/manifest.json"), undefined, "file listing must remain fresh");
  ignoresRange = true;
  const badChunk = source + "&range=4-7";
  const beforeBadRange = calls.length;
  await request(badChunk);
  await request(badChunk);
  assert.equal(calls.length, beforeBadRange + 2, "a host ignoring Range must not poison the chunk cache");
  delayWrite = true;
  const responsive = await Promise.race([
    request(source + "&write-test=1", base, false),
    new Promise((_, reject) => setTimeout(() => reject(new Error("cache write blocked the response")), 1000)),
  ]);
  assert.equal(await responsive.text(), "preview");
  assert.equal(typeof finishWrite, "function");
  finishWrite();
  delayWrite = false;
  offline = true;
  assert.ok(await request("cyrus-config/cache/viewer/catalog.json"), "cached catalog remains available offline");
  assert.equal(request("cyrus-config/menu.json"), undefined, "local settings must not enter asset caching");
  offline = false;
  brokenBodies = 1;
  const beforeInterrupted = calls.length;
  const recovered = await request("cyrus-config/cache/world.glb");
  assert.equal(await recovered.text(), "preview", "an interrupted asset body is downloaded again");
  assert.equal(calls.length, beforeInterrupted + 2);
  assert.equal(await saved.get(base + "cyrus-config/cache/world.glb").text(), "preview", "only complete bytes enter the cache");
  brokenBodies = 3;
  const beforePermanent = calls.length;
  await assert.rejects(request("cyrus-config/cache/broken.glb"), /connection reset/);
  assert.equal(calls.length, beforePermanent + 3, "permanent download failures stop after three attempts");
  assert.equal(saved.has(base + "cyrus-config/cache/broken.glb"), false);
  const remote = await request(immutable, "https://other.test/");
  assert.equal(await remote.text(), "preview", "remote install media uses the shared cache too");
  console.log("shared web cache contracts passed");
})().catch(error => { console.error(error); process.exitCode = 1; });
