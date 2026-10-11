// Bevy owns the output context; resume it inside the browser's trusted input event.
if (globalThis.AudioContext) {
  const contexts = new Set();
  const AudioContext = globalThis.AudioContext;
  globalThis.AudioContext = class extends AudioContext {
    constructor(...args) {
      super(...args);
      contexts.add(this);
      this.addEventListener("statechange", () => {
        if (this.state === "closed") contexts.delete(this);
      });
    }
  };
  const resume = () => {
    for (const context of contexts) {
      if (context.state === "suspended") {
        context.resume().catch(error => console.warn("Audio resume failed", error));
      }
    }
  };
  for (const event of ["pointerup", "keydown", "touchend"]) {
    document.addEventListener(event, resume, { capture: true, passive: true });
  }
}

const label = document.getElementById("label");
const fill = document.getElementById("fill");
const engine = document.body.dataset.engine;
const scene = new URLSearchParams(location.search).get("scene")?.toUpperCase();
try {
  if (scene && !/^[A-Z0-9_]+$/.test(scene)) throw new Error("Invalid scene name");
  if ("serviceWorker" in navigator) {
    try {
      await navigator.serviceWorker.register("./sw.js");
      await navigator.serviceWorker.ready;
    } catch (error) { console.warn("Asset cache unavailable", error); }
  }
  async function download(url, name) {
    const response = await fetch(url, { cache: "no-cache" });
    if (!response.ok) throw new Error(name + ": HTTP " + response.status);
    const total = Number(response.headers.get("content-length")) || 0;
    const reader = response.body.getReader(), chunks = [];
    let received = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value); received += value.length;
      fill.style.width = total ? (100 * received / total) + "%" : "0%";
      label.textContent = name + " · " + (received / 1048576).toFixed(1) + " MB" +
        (total ? " / " + (total / 1048576).toFixed(1) + " MB" : "");
    }
    return { bytes: await new Blob(chunks).arrayBuffer(),
      version: encodeURIComponent(response.headers.get("last-modified") || Date.now()) };
  }
  const wasm = await download("./pkg/" + engine + "_bg.wasm", "Loading application");
  if (engine === "cyrus-web" && scene) {
    await download("./cyrus-config/cache/" + scene + ".glb", "Loading " + scene);
  }
  label.textContent = "Starting application…";
  const { default: init } = await import("./pkg/" + engine + ".js?v=" + wasm.version);
  try { await init({ module_or_path: wasm.bytes }); }
  catch (error) { if (!String(error).includes("Using exceptions for control flow")) throw error; }
  if (document.body.dataset.loading !== "application") {
    document.getElementById("loading").remove();
  }
} catch (error) {
  label.textContent = "Failed: " + error.message;
  fill.style.background = "#c66";
  console.error(error);
}
