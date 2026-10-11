const FILE_CACHE = "cyrus-assets-v2";
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", event => event.waitUntil(self.clients.claim()));
self.addEventListener("fetch", event => {
  const url = new URL(event.request.url);
  const scope = new URL("./", self.location.href).pathname;
  const relative = url.pathname.slice(scope.length);
  const original = relative.startsWith("original/") && /^[a-f0-9]{64}$/i.test(url.searchParams.get("v") || "");
  if (event.request.method !== "GET" || url.origin !== self.location.origin ||
      !(relative.startsWith("cyrus-config/cache/") || relative.endsWith(".wasm") || original)) return;
  event.respondWith((async () => {
    const cache = await caches.open(FILE_CACHE);
    const key = original ? new Request(url.href) : event.request;
    const cached = await cache.match(key);
    // Viewer generation paths are immutable; the catalog and game exports are revalidated.
    if (cached && original) return cached;
    if (cached && /^cyrus-config\/cache\/viewer\/[a-f0-9]{64}\//.test(relative)) return cached;
    if (cached) {
      try {
        const head = await fetch(event.request.url, { method: "HEAD", cache: "no-store" });
        const modified = head.headers.get("last-modified"), size = head.headers.get("content-length");
        if (head.ok && modified && size && modified === cached.headers.get("last-modified") &&
            size === cached.headers.get("content-length")) return cached;
      } catch { return cached; }
    }
    const response = await fetch(event.request, { cache: "no-store" });
    const range = original && url.searchParams.get("range");
    const validRange = !range || response.headers.get("content-range")?.startsWith("bytes " + range + "/");
    if (response.ok && validRange) {
      try {
        // Cache Storage rejects 206; the digest and explicit range identify this chunk.
        const stored = original && response.status === 206
          ? new Response(await response.clone().arrayBuffer(), { headers: response.headers })
          : response.clone();
        await cache.put(key, stored);
      }
      catch (error) { console.warn("Asset cache write failed", error); }
    }
    return response;
  })());
});
