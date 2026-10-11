// Exercise the built WASM transport with synthetic original bytes, without a browser/install.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const packageDir = path.resolve(process.argv[2] || "target/web/pkg");
const { default: init, AssetLibrary } = await import(pathToFileURL(path.join(packageDir, "cyrus-assets-worker.js")));
const pcm = Buffer.alloc(3 * 1024 * 1024, 128);
const payload = Buffer.alloc(6 + 5 + 27 + pcm.length);
payload[1] = 1;
payload.writeUInt16LE(5, 2);
payload.write("voice", 6);
payload.writeUInt32LE(11025, 11 + 8);
payload.writeUInt32LE(2 ** 32 - 1, 11 + 18);
payload.writeUInt32LE(pcm.length, 11 + 22);
pcm.copy(payload, 11 + 27);
const original = Buffer.alloc(8 + payload.length + 12 + 12);
original.write("TEST", 0);
original.writeUInt32BE(payload.length, 4);
payload.copy(original, 8);
const index = 8 + payload.length;
original.write("TEST", index);
original.writeUInt32LE(8, index + 4);
original.writeUInt32LE(payload.length, index + 8);
original.write("RNAV", index + 12);
original.writeUInt32LE(index, index + 16);
original.writeUInt32LE(1, index + 20);
const requests = [];
let malformed = false;
let transportFailures = 1;
globalThis.XMLHttpRequest = class {
  open(method, url, async) {
    assert.equal(method, "GET");
    assert.equal(async, false);
    this.url = new URL(url);
  }
  setRequestHeader(name, value) { assert.equal(name, "Range"); this.range = value; }
  getResponseHeader(name) {
    assert.equal(name, "Content-Range");
    return malformed ? null : this.contentRange;
  }
  send() {
    if (transportFailures > 0) { transportFailures--; throw new Error("temporary transport failure"); }
    assert.equal(this.timeout, 30000);
    const match = /^bytes=(\d+)-(\d+)$/.exec(this.range);
    assert.ok(match, "large originals must use ranges");
    const [start, end] = match.slice(1).map(Number);
    assert.equal(this.url.searchParams.get("range"), `${start}-${end}`);
    assert.ok(end - start + 1 <= 1024 * 1024, "requests must stay within 1 MiB");
    assert.ok(end < original.length);
    requests.push([start, end]);
    this.status = 206;
    this.contentRange = `bytes ${start}-${end}/${original.length}`;
    this.response = original.buffer.slice(original.byteOffset + start, original.byteOffset + end + 1);
  }
};
await init({ module_or_path: fs.readFileSync(path.join(packageDir, "cyrus-assets-worker_bg.wasm")) });
const manifest = JSON.stringify({ files: ["ENGLISH.RTX", "fxart/TEST.ROB", "fxart/TEXBSI.001", "TEST.SFX"].map(path => ({
  path, bytes: original.length, digest: "a".repeat(64),
})) });
const progress = [];
const library = new AssetLibrary("https://example.test/original/", manifest, (...args) => progress.push(args));
assert.deepEqual(requests, [], "startup must not read archive bytes");
assert.ok(JSON.parse(library.catalog()).entries.every(entry => entry.source === "Archive"));
library.expand("ENGLISH.RTX");
assert.deepEqual(requests, [[original.length - 12, original.length - 1], [index, index + 11]], "catalog reads only footer and directory");
const catalog = JSON.parse(library.catalog());
const clip = catalog.entries.find(entry => entry.source.Speech === 0);
assert.equal(clip.name, "TEST");
assert.equal(clip.category, "Speech");
const preview = library.preview(clip.id);
assert.ok(progress.some(([done, total, name]) => done > 0 && done < total && name.includes("MiB")), "selected resource reports intermediate progress");
assert.ok(JSON.parse(preview.preview).content.Audio);
assert.deepEqual(Buffer.from(preview.files[0][1]).subarray(44), pcm, "chunk boundaries preserve the audio bytes");
library.free();
for (const sourcePath of ["ENGLISH.RTX", undefined]) {
  requests.length = 0;
  const linked = new AssetLibrary("https://example.test/original/", manifest, () => {});
  const resolved = JSON.parse(linked.resolve(clip.id, sourcePath));
  assert.ok(resolved.entries.some(entry => entry.id === clip.id), "new and legacy links find the member");
  assert.deepEqual(requests, [[original.length - 12, original.length - 1], [index, index + 11]]);
  linked.free();
}
malformed = true;
const broken = new AssetLibrary("https://example.test/original/", manifest, () => {});
assert.throws(() => broken.expand("ENGLISH.RTX"), /server must support byte-range requests/);
broken.free();
console.log("WASM index-only startup, selected speech, chunking, progress and range validation passed");
