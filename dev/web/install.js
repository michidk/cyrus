import { localInstall, directoryEntries } from './local-install.js';

// All browser applications share the selected read-only installation source.
const STORAGE_KEY = "cyrus-install";

export function directoryURL(value, base) {
  const url = new URL(value, base);
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) {
    throw new Error("Use an HTTP or HTTPS game-files URL without credentials.");
  }
  url.search = "";
  url.hash = "";
  if (!url.pathname.endsWith("/")) url.pathname += "/";
  return url;
}

export async function validateInstall(value, { base = location.href, fetcher = fetch } = {}) {
  const url = directoryURL(value, base);
  const response = await fetcher(new URL("manifest.json", url), { cache: "no-store" });
  if (!response.ok) throw new Error("Game files: HTTP " + response.status);
  const manifest = await response.json();
  if (!Array.isArray(manifest.files) || !manifest.files.length) throw new Error("No game files found at this URL.");
  const paths = manifest.files.map(file => file.path?.toLowerCase());
  if (!paths.includes("rgfx.exe") || !paths.some(path => path?.startsWith("fxart/")) ||
      !paths.some(path => path?.startsWith("maps/"))) throw new Error("This URL is not a Redguard install.");
  const probes = [
    manifest.files.find(file => file.path?.toLowerCase() === "rgfx.exe"),
    manifest.files.find(file => file.path?.toLowerCase().startsWith("fxart/")),
    manifest.files.find(file => file.path?.toLowerCase().startsWith("maps/")),
  ];
  await Promise.all(probes.map(async file => {
    const available = await fetcher(new URL(file.path, url), { method: "HEAD", cache: "no-store" });
    if (!available.ok) throw new Error(file.path + ": game files are no longer available: HTTP " + available.status);
  }));
  return { url: url.href };
}

export async function chooseInstall() {
  const params = new URLSearchParams(location.search);
  let saved;
  try { saved = localStorage.getItem(STORAGE_KEY); } catch { /* Storage may be disabled. */ }
  let defaults = {};
  try {
    const response = await fetch("./launch.json", { cache: "no-store" });
    if (response.ok) defaults = await response.json();
  } catch { /* Static hosting has no server defaults. */ }
  const local = params.get("install") === "local" || (!params.has("install") && saved === "local");
  const candidate = local ? null : params.has("install") ? params.get("install") : defaults.install || saved;
  let message = "";
  async function accept(value) {
    const source = await validateInstall(value);
    for (const [name, content] of [["cyrus-assets", source.url]]) {
      if (!content) continue;
      let element = document.querySelector(`meta[name="${name}"]`);
      if (!element) {
        element = document.createElement("meta");
        element.name = name;
        document.head.append(element);
      }
      element.content = content;
    }
    try { localStorage.setItem(STORAGE_KEY, source.url); } catch { /* Selection still works without persistence. */ }
    return source;
  }
  if (candidate) {
    try { return await accept(candidate); }
    catch (error) { message = error.message; }
  }
  const form = document.createElement("form");
  form.id = "install-picker";
  const folder = document.createElement("button");
  folder.type = "button";
  folder.textContent = "Choose game folder on your computer";
  const folderInput = document.createElement("input");
  folderInput.type = "file";
  folderInput.id = "install-folder";
  folderInput.webkitdirectory = true;
  folderInput.multiple = true;
  folderInput.hidden = true;
  const localHelp = document.createElement("p");
  localHelp.textContent = "Choose the GOG folder containing Redguard, DOSBOX and game.gog. Files stay on your computer and are not uploaded. Select the folder again after reloading.";
  const label = document.createElement("label");
  label.textContent = "Or connect to a game-file server";
  const input = document.createElement("input");
  input.type = "url";
  input.required = true;
  input.id = "install-url";
  input.placeholder = "https://your-private-server/original/";
  input.value = candidate ? directoryValue(candidate) : "";
  label.htmlFor = input.id;
  const help = document.createElement("p");
  help.textContent = "Use this option if you already run the included Cyrus server for your game files. Enter that server's address, not a game download link. Otherwise, choose a folder on your computer above.";
  const error = document.createElement("p");
  error.setAttribute("role", "alert");
  error.textContent = message;
  const submit = document.createElement("button");
  submit.textContent = "Connect to server";
  form.append(folder, folderInput, localHelp, label, input, help, error, submit);
  document.getElementById("loading").append(form);
  document.getElementById("label").textContent = "Choose your Redguard game files";
  folder.focus();
  return new Promise(resolve => {
    function acceptLocal(entries) {
      const source = localInstall(entries);
      const url = new URL(location.href);
      url.searchParams.set("install", "local");
      history.replaceState(null, "", url.href);
      try { localStorage.setItem(STORAGE_KEY, "local"); } catch { /* Selection works without storage. */ }
      document.querySelector('meta[name="cyrus-assets"]')?.remove();
      form.remove();
      resolve(source);
    }
    function busy(value) { folder.disabled = submit.disabled = input.disabled = value; }
    folder.addEventListener("click", async () => {
      if (!globalThis.showDirectoryPicker) { folderInput.click(); return; }
      busy(true);
      error.textContent = "";
      try {
        const directory = await showDirectoryPicker({ mode: "read" });
        error.textContent = "Checking selected folder…";
        acceptLocal(await directoryEntries(directory));
      } catch (failure) {
        if (failure.name !== "AbortError") error.textContent = failure.message;
      } finally { busy(false); }
    });
    folderInput.addEventListener("change", () => {
      if (!folderInput.files.length) return;
      try { acceptLocal([...folderInput.files].map(file => [file.webkitRelativePath, file])); }
      catch (failure) { error.textContent = failure.message; folderInput.value = ""; }
    });
    form.addEventListener("submit", async event => {
      event.preventDefault();
      busy(true);
      error.textContent = "Checking game files…";
      try {
        const source = await accept(input.value);
        const url = new URL(location.href);
        url.searchParams.set("install", source.url);
        history.replaceState(null, "", url.href);
        form.remove();
        resolve(source);
      } catch (failure) {
        error.textContent = failure.message;
        busy(false);
      }
    });
  });
}

function directoryValue(value) {
  try { return directoryURL(value, location.href).href; } catch { return value; }
}
