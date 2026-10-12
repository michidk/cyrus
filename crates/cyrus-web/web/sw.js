const GAME_FILE_CACHE = "cyrus-game-files-v1";

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", event => event.waitUntil(self.clients.claim()));

self.addEventListener("fetch", event => {
  const url = new URL(event.request.url);
  const cacheable = url.pathname.endsWith(".glb") || url.pathname.endsWith(".wasm");
  if (event.request.method !== "GET" || url.origin !== self.location.origin || !cacheable) return;

  event.respondWith((async () => {
    const cache = await caches.open(GAME_FILE_CACHE);
    const cached = await cache.match(event.request);

    if (cached) {
      try {
        const head = await fetch(event.request.url, { method: "HEAD", cache: "no-store" });
        const modified = head.headers.get("last-modified");
        const size = head.headers.get("content-length");
        // shortcut: timestamp plus size identifies an export; use content hashes if same-second rebuilds become common.
        if (head.ok && modified && size &&
            modified === cached.headers.get("last-modified") &&
            size === cached.headers.get("content-length")) return cached;
      } catch {
        return cached;
      }
    }

    const response = await fetch(event.request, { cache: "no-store" });
    if (response.ok) {
      // Consume both branches together instead of buffering the entire game file before returning it.
      event.waitUntil(cache.put(event.request, response.clone()).catch(error => {
        console.warn("Download cache unavailable", error);
      }));
    }
    return response;
  })());
});
