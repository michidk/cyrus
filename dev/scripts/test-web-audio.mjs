import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const host = new EventTarget();
const notices = new Map();
host.getElementById = id => notices.get(id);
host.createElement = () => ({ style: {}, setAttribute(name, value) { this[name] = value; } });
host.body = { append: notice => notices.set(notice.id, notice) };
let resumes = 0;
host.AudioContext = class extends EventTarget {
  state = "suspended";
  resume() {
    resumes++;
    this.state = "running";
    return Promise.resolve();
  }
};
vm.runInNewContext((await readFile(new URL("../web/loader.js", import.meta.url), "utf8")).split("const label")[0], { globalThis: host, document: host, window: host, console });
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
const failure = new Event("error");
failure.error = new Error("World renderer failed");
host.dispatchEvent(failure);
assert.equal(notices.get("failure").role, "alert");
assert.equal(notices.get("failure").textContent, "Game stopped: World renderer failed\nReload the page to try again.");
host.dispatchEvent(new Event("webglcontextlost"));
assert.equal(notices.size, 1);
assert.equal(notices.get("failure").textContent, "Game stopped: Graphics context lost\nReload the page to try again.");
const rejection = new Event("unhandledrejection");
rejection.reason = "Asset request failed";
host.dispatchEvent(rejection);
assert.equal(notices.get("failure").textContent, "Game stopped: Asset request failed\nReload the page to try again.");
context.state = "closed";
host.dispatchEvent(new Event("keydown"));
assert.equal(resumes, 2);
