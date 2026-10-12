const assert = require("node:assert/strict");
const { test } = require("node:test");
const fs = require("node:fs/promises");
const { pathToFileURL } = require("node:url");
const os = require("node:os");
const path = require("node:path");

test("install URLs validate original files and game preparation independently of the app host", async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), "cyrus-install-"));
  try {
    const modulePath = path.join(directory, "install.mjs");
    await fs.copyFile(path.join(__dirname, "install.js"), modulePath);
    const { directoryURL, validateInstall, chooseInstall } = await import(pathToFileURL(modulePath));
    const base = "https://app.test/viewer.html";
    assert.equal(directoryURL("https://files.test/original", base).href, "https://files.test/original/");
    assert.throws(() => directoryURL("file:///game", base));
    assert.throws(() => directoryURL("https://user:password@files.test/", base));
    const calls = [];
    const files = ["RGFX.EXE", "fxart/A.ROB", "maps/START.RGM"].map(path => ({ path }));
    let available = true;
    const fetcher = async (url, options) => {
      calls.push([url.href, options]);
      return options.method === "HEAD" ? new Response(null, { status: available ? 200 : 404 })
        : Response.json({ files, cache: "../cyrus-config/cache/" });
    };
    const source = await validateInstall("https://files.test/original", { base, game: true, fetcher });
    assert.equal(source.url, "https://files.test/original/");
    assert.equal(source.cache, "https://files.test/cyrus-config/cache/");
    assert.equal(calls[1][0], "https://files.test/original/RGFX.EXE");
    assert.equal(calls[1][1].cache, "no-store");
    available = false;
    await assert.rejects(validateInstall(source.url, { base, fetcher }), /no longer available/);
    const staticFiles = async (_, options) => options.method === "HEAD" ? new Response(null) : Response.json({ files });
    await validateInstall(source.url, { base, fetcher: staticFiles });
    await assert.rejects(validateInstall(source.url, { base, game: true, fetcher: staticFiles }), /preparation/);
    await assert.rejects(validateInstall(source.url, { base, fetcher: async () => Response.json({ files: [] }) }), /No game files/);
    const original = Object.getOwnPropertyDescriptors(globalThis);
    try {
      const values = new Map([["cyrus-install", "https://saved.test/original/"]]);
      globalThis.location = { href: base, search: "" };
      globalThis.localStorage = { getItem: key => values.get(key), setItem: (key, value) => values.set(key, value) };
      globalThis.sessionStorage = { getItem: () => null, removeItem: () => {} };
      const metas = [];
      globalThis.document = {
        body: { dataset: { engine: "cyrus-web" } },
        querySelector: () => null,
        createElement: () => ({}),
        head: { append: element => metas.push(element) },
      };
      const selected = [];
      let defaults = {};
      globalThis.fetch = async (url, options) => {
        if (url === "./launch.json") return Response.json(defaults);
        selected.push(String(url));
        return options.method === "HEAD" ? new Response(null)
          : Response.json({ files, cache: "../cyrus-config/cache/", worlds: ["START", "27"] });
      };
      assert.equal((await chooseInstall()).url, "https://saved.test/original/");
      assert.equal(metas.find(meta => meta.name === "cyrus-cache").content, "https://saved.test/cyrus-config/cache/");
      defaults = { install: "https://server.test/original/" };
      assert.equal((await chooseInstall()).url, "https://server.test/original/", "server defaults bypass the picker");
      globalThis.location.search = "?install=https%3A%2F%2Fexplicit.test%2Foriginal%2F";
      assert.equal((await chooseInstall()).url, "https://explicit.test/original/", "URL overrides the server default");
      assert.equal(values.get("cyrus-install"), "https://explicit.test/original/");
      assert.ok(selected.every(url => !url.includes("app.test")), "game data uses the selected host");
    } finally {
      for (const key of ["document", "location", "localStorage", "sessionStorage", "fetch"]) {
        if (original[key]) Object.defineProperty(globalThis, key, original[key]); else delete globalThis[key];
      }
    }

  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});
