import { chooseInstall } from "./install.js";

// Resume playback contexts inside the browser's trusted input event.
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
      if (context.state === "suspended" || context.state === "interrupted") {
        context.resume().catch(error => console.warn("Audio resume failed", error));
      }
    }
  };
  for (const event of ["pointerup", "keydown", "touchend"]) {
    document.addEventListener(event, resume, { capture: true, passive: true });
  }
}

function reportFailure(error) {
  let notice = document.getElementById("failure");
  if (!notice) {
    notice = document.createElement("div");
    notice.id = "failure";
    notice.setAttribute("role", "alert");
    notice.style.cssText = "position:fixed;top:0;left:0;right:0;padding:12px;background:#000;color:#fff;white-space:pre-wrap;z-index:100";
    document.body.append(notice);
  }
  notice.textContent = `Game stopped: ${error?.message || String(error)}\nReload the page to try again.`;
}
window.addEventListener("error", event => reportFailure(event.error || event.message));
window.addEventListener("unhandledrejection", event => reportFailure(event.reason));
document.addEventListener("webglcontextlost", () => reportFailure("Graphics context lost"), true);

const label = document.getElementById("label");
const fill = document.getElementById("fill");
const engine = document.body.dataset.engine;
const scene = new URLSearchParams(location.search).get("scene")?.toUpperCase();
try {
  if (scene !== undefined && !/^[A-Z0-9_]+$/.test(scene)) throw new Error("Invalid scene name");
  if (engine === "cyrus-web" || engine === "cyrus-viewer") {
    const params = new URLSearchParams(location.search);
    if (engine === "cyrus-web") {
      for (const area of ["water", "fog", "sky"]) {
        if (params.has(area) && !["faithful", "modern"].includes(params.get(area))) throw new Error(area + ": expected faithful or modern");
      }
      if (params.has("skip-startup") && !["", "true", "false", "1", "0"].includes(params.get("skip-startup"))) throw new Error("skip-startup: expected true or false");
    } else if (!location.hash && params.has("asset")) {
      const selection = ["asset", "file", "palette"].filter(name => params.has(name))
        .map(name => name + "=" + encodeURIComponent(params.get(name))).join("&");
      history.replaceState(null, "", location.pathname + location.search + "#" + selection);
    } else if (!params.has("asset") && (params.has("file") || params.has("palette"))) {
      throw new Error("file and palette require asset");
    }
    const source = await chooseInstall();
    globalThis.cyrusInstallSource = source.files || source.url;
    if (engine === "cyrus-web") {
      const { openGameAssets } = await import('./game-assets.js');
      globalThis.cyrusGameAssets = await openGameAssets(globalThis.cyrusInstallSource, { progress(done, total, name) {
        if (!label.isConnected) return;
        label.textContent = name;
        fill.classList.toggle('indeterminate', !total);
        fill.style.width = total ? (100 * done / total) + '%' : '0%';
      } });
      globalThis.cyrusCheatSeed = new TextDecoder().decode(await globalThis.cyrusGameAssets.read('cheats/startup.json'));
      document.querySelector('meta[name=cyrus-worlds]').content = globalThis.cyrusGameAssets.worlds.join(',');
    }
  }
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
      fill.classList.toggle("indeterminate", !total);
      fill.style.width = total ? (100 * received / total) + "%" : "0%";
      label.textContent = name + " · " + (received / 1048576).toFixed(1) + " MB" +
        (total ? " / " + (total / 1048576).toFixed(1) + " MB" : "");
    }
    return { bytes: await new Blob(chunks).arrayBuffer(),
      version: encodeURIComponent(response.headers.get("last-modified") || Date.now()) };
  }
  const wasm = await download("./pkg/" + engine + "_bg.wasm", "Loading application");
  label.textContent = "Starting application…";
  fill.classList.add("indeterminate");
  const { default: init } = await import("./pkg/" + engine + ".js?v=" + wasm.version);
  try { await init({ module_or_path: wasm.bytes }); }
  catch (error) { if (!String(error).includes("Using exceptions for control flow")) throw error; }
  if (document.body.dataset.loading !== "application") {
    document.getElementById("loading").remove();
  }
} catch (error) {
  label.textContent = "Failed: " + error.message;
  fill.classList.remove("indeterminate");
  fill.style.background = "#c66";
  console.error(error);
}
