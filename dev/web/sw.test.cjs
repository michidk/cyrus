// Shared-cache contract without browser or game-data fixtures.
const assert = require("node:assert/strict");
const vm = require("node:vm");
const fs = require("node:fs");
const events = {};
const saved = new Map();
const calls = [];
let offline = false;
let ignoresRange = false;
const base = "https://example.test/tools/";
const context = {
  URL, console, Request, Response,
  self: { location: new URL(base + "sw.js"), addEventListener: (name, fn) => events[name] = fn },
  caches: { open: async () => ({ match: async req => saved.get(req.url), put: async (req, value) => saved.set(req.url, value) }) },
  fetch: async (request, options) => {
    calls.push([typeof request === "string" ? request : request.url, options]);
    if (offline) throw new Error("offline");
    if ((typeof request === "string" ? request : request.url).includes("range=")) {
      if (ignoresRange) return new Response("whole file");
      return new Response("part", { status: 206, headers: { "content-range": "bytes 0-3/8" } });
    }
    return new Response("preview", { headers: { "last-modified": "today", "content-length": "7" } });
  },
};
vm.runInNewContext(fs.readFileSync(__dirname + "/sw.js", "utf8"), context);
function request(path, origin = base) {
  let response;
  events.fetch({ request: { method: "GET", url: origin + path }, respondWith: promise => response = promise });
  return response;
}
(async () => {
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
  offline = true;
  assert.ok(await request("cyrus-config/cache/viewer/catalog.json"), "cached catalog remains available offline");
  assert.equal(request("cyrus-config/menu.json"), undefined, "local settings must not enter asset caching");
  assert.equal(request(immutable, "https://other.test/"), undefined);
  console.log("shared web cache contracts passed");
})().catch(error => { console.error(error); process.exitCode = 1; });
