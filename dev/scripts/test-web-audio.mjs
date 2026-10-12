import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const host = new EventTarget();
let resumes = 0;
host.AudioContext = class extends EventTarget {
  state = "suspended";
  resume() {
    resumes++;
    this.state = "running";
    return Promise.resolve();
  }
};
vm.runInNewContext((await readFile(new URL("../web/loader.js", import.meta.url), "utf8")).split("function reportFailure")[0], { globalThis: host, document: host, console });
const context = new host.AudioContext();
assert.equal(resumes, 0);
host.dispatchEvent(new Event("keydown"));
assert.equal(context.state, "running");
assert.equal(resumes, 1);
host.dispatchEvent(new Event("pointerup"));
assert.equal(resumes, 1);
context.state = "interrupted";
host.dispatchEvent(new Event("pointerup"));
assert.equal(context.state, "running");
assert.equal(resumes, 2);
context.state = "closed";
host.dispatchEvent(new Event("keydown"));
assert.equal(resumes, 2);
