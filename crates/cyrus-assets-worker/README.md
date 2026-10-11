# cyrus-assets-worker

Worker-only WASM adapter for the shared `cyrus-assets` runtime parsers.
The browser fetches an original-file manifest, then reads original bytes over
HTTP. Synchronous reads run exclusively inside the worker so preservation's
seekable parsers do not block the viewer UI.

The file host must support byte ranges for the CD image. The manifest lists
relative paths, byte lengths and content digests. Generated previews and media
are transferred to the viewer in memory. No game files are required to build.

Build and package with `just viewer-web-build`. The worker script is
`dev/web/asset-worker.js`.
