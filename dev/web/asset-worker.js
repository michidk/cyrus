import init, { AssetLibrary, GameLibrary } from "./pkg/cyrus-assets-worker.js";

let library, game;
self.onmessage = async ({ data }) => {
  const { kind, id } = data;
  try {
    if (kind === "game-open") {
      await init();
      const base = new URL(data.source, self.location.href);
      const response = await fetch(new URL("manifest.json", base), { cache: "no-cache" });
      if (!response.ok) throw new Error("Original-file listing: HTTP " + response.status);
      game = new GameLibrary(base.href, await response.text(), (done, total, name) => {
        self.postMessage({ kind: "progress", done, total, name });
      });
      self.postMessage({ kind, id, worlds: game.worlds() });
    } else if (kind === "game-prepare") {
      if (!game) throw new Error("Game source is not ready");
      const result = game.prepare(data.path);
      self.postMessage({ kind, id, ...result }, result.files.map(([, bytes]) => bytes.buffer));
    } else if (kind === "movie-range") {
      if (!game) throw new Error("Game source is not ready");
      const result = game.movie_range(data.index, data.start, data.length);
      self.postMessage({ kind, id, ...result }, [result.bytes.buffer]);
    } else if (kind === "open") {
      await init();
      const base = new URL(data.source, self.location.href);
      const response = await fetch(new URL("manifest.json", base), { cache: "no-cache" });
      if (!response.ok) throw new Error("Original-file listing: HTTP " + response.status);
      library = new AssetLibrary(base.href, await response.text(), (done, total, name) => {
        self.postMessage({ kind: "progress", done, total, name });
      });
      self.postMessage({ kind: "catalog", catalog: library.catalog() });
    } else if (kind === "expand" || kind === "resolve") {
      if (!library) throw new Error("Asset library is not ready");
      const selection = kind === "resolve" ? JSON.parse(data.selection) : null;
      const catalog = kind === "expand" ? library.expand(data.path) : library.resolve(selection.id, selection.path);
      self.postMessage({ kind: "catalog", catalog });
    } else if (kind === "preview") {
      if (!library) throw new Error("Asset library is not ready");
      const result = library.preview(id, data.palette);
      self.postMessage({ kind, id, ...result }, result.files.map(([, bytes]) => bytes.buffer));
    } else {
      throw new Error("Unknown asset-worker request");
    }
  } catch (error) {
    self.postMessage({ kind: "error", id, message: String(error) });
  }
};
