const assert = require("node:assert/strict");
const { test } = require("node:test");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { pathToFileURL } = require("node:url");

test("browser loader hands its existing overlay to the application", async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "cyrus-loader-"));
  const original = Object.getOwnPropertyDescriptors(globalThis);
  try {
    await fs.mkdir(path.join(directory, "pkg"));
    await fs.copyFile(path.join(__dirname, "loader.js"), path.join(directory, "loader.mjs"));
    await fs.writeFile(path.join(directory, "install.js"), "export async function chooseInstall() {}\nexport function installControls() {}\n");
    await fs.writeFile(path.join(directory, "package.json"), '{"type":"module"}');
    await fs.writeFile(path.join(directory, "pkg/test.js"), "export default async () => {};\n");
    globalThis.location = { search: "" };
    globalThis.window = { addEventListener() {} };
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: {} });
    globalThis.fetch = async () => new Response(new Uint8Array([0]), {
      headers: { "content-length": "1", "last-modified": "test" },
    });
    for (const application of [true, false]) {
      const listeners = {};
      let resumes = 0;
      globalThis.AudioContext = class extends EventTarget {
        state = "suspended";
        async resume() { resumes++; this.state = "running"; }
      };
      let removed = false;
      const elements = {
        loading: { remove: () => { removed = true; } },
        label: { textContent: "" },
        fill: { style: {}, classList: { toggle() {}, add() {} } },
      };
      globalThis.document = {
        addEventListener: (event, callback) => { listeners[event] = callback; },
        body: { dataset: { engine: "test", loading: application ? "application" : undefined } },
        getElementById: id => elements[id],
      };
      await import(pathToFileURL(path.join(directory, "loader.mjs")) + "?application=" + application);
      const context = new globalThis.AudioContext();
      assert.equal(resumes, 0, "audio must wait for a user gesture");
      listeners.pointerup();
      assert.equal(resumes, 1, "a click resumes suspended audio");
      listeners.keydown();
      assert.equal(resumes, 1, "running contexts are left alone");
      context.state = "suspended";
      listeners.touchend();
      assert.equal(resumes, 2, "later browser suspension can recover");
      context.state = "closed";
      context.dispatchEvent(new Event("statechange"));
      context.state = "suspended";
      listeners.pointerup();
      assert.equal(resumes, 2, "closed contexts are forgotten");
      assert.equal(removed, !application, "only the application may hide its loading overlay");
      assert.equal(elements.label.textContent, "Starting application…");
      assert.equal(elements.fill.style.width, "100%");
    }
    await fs.writeFile(path.join(directory, "pkg/cyrus-viewer.js"), "export default async () => {};\n");
    globalThis.location = { pathname: "/viewer.html", hash: "", search: "?asset=abc&file=source%20path&palette=night%20palette" };
    let replaced;
    globalThis.history = { replaceState: (_, __, url) => { replaced = url; } };
    document.body.dataset.engine = "cyrus-viewer";
    await import(pathToFileURL(path.join(directory, "loader.mjs")) + "?selection=true");
    assert.equal(replaced, "/viewer.html?asset=abc&file=source%20path&palette=night%20palette#asset=abc&file=source%20path&palette=night%20palette");
  } finally {
    for (const key of ["window", "document", "location", "history", "navigator", "fetch", "AudioContext"]) {
      if (original[key]) Object.defineProperty(globalThis, key, original[key]);
      else delete globalThis[key];
    }
    await fs.rm(directory, { recursive: true, force: true });
  }
});
