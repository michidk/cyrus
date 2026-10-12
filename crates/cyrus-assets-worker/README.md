# cyrus-assets-worker

Worker-only WASM adapter for the shared `cyrus-assets` runtime parsers.
The browser fetches an original-file manifest, then reads original bytes over
HTTP. Synchronous reads run exclusively inside the worker so preservation's
seekable parsers do not block game or viewer UI.

The file host must support byte ranges. Downloads use at most 1 MiB per request,
report progress for large files, and time out after 30 seconds per request.
The manifest lists relative paths, byte lengths and content digests. Generated previews and media
are transferred to the game or viewer in memory. `GameLibrary` prepares cache
groups through the shared byte-source exporters and reads bounded cinematic ranges. No game files are required to build.

Build and package with `just viewer-web-build`. The worker script is
`dev/web/asset-worker.js`. Verify the built worker without an install using
`node dev/scripts/test-assets-worker.mjs`.

With an install linked, run `node dev/scripts/test-game-worker.mjs` against the
built package to check game exports, preserved actor metadata and bounded CD movie
reads without a native executable or prepared cache.
