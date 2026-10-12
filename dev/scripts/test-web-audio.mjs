import assert from "node:assert/strict";
import { unlockAudio } from "../../crates/cyrus-web/web/audio.js";

const host = new EventTarget();
let resumes = 0;
host.AudioContext = class {
  state = "suspended";
  resume() {
    resumes++;
    this.state = "running";
    return Promise.resolve();
  }
};
unlockAudio(host);
const context = new host.AudioContext();
assert.equal(resumes, 0);
host.dispatchEvent(new Event("keydown"));
assert.equal(context.state, "running");
assert.equal(resumes, 1);
host.dispatchEvent(new Event("pointerdown"));
assert.equal(resumes, 1);
context.state = "interrupted";
host.dispatchEvent(new Event("pointerdown"));
assert.equal(context.state, "running");
assert.equal(resumes, 2);
context.state = "closed";
host.dispatchEvent(new Event("keydown"));
assert.equal(resumes, 2);
