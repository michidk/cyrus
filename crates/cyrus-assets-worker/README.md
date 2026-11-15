# cyrus-assets-worker

WASM adapter that runs the shared `cyrus-assets` parsers inside a browser Web
Worker, so preservation's synchronous, seekable parsers never block the game or
viewer UI. The page supplies either an HTTP install URL with a manifest
(relative paths, byte lengths, content digests) or the player's selected local
`File` objects. Generated previews and media go back to the game or viewer in
memory. It depends only on `cyrus-assets` (with `parsers`) and compiles to
nothing on native targets. No game files are needed to build it.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `AssetLibrary` (viewer catalog and previews) and `GameLibrary` (game cache groups, bounded cinematic ranges); HTTP range reads |
| [local](src/local.rs) | Read-only local `File` handles with bounded reads and content fingerprints |

The JavaScript side is [`dev/web/asset-worker.js`](../../dev/web/asset-worker.js).

## Notes

- Local files are read in place: no network requests, uploads or copies into
  browser storage.
- Reads and fingerprints use 1 MiB chunks. The HTTP file host must support byte
  ranges; each request times out after 30 seconds.

## Testing

`dev/scripts/build-web.sh` builds the worker together with the game or viewer
(`just web-build`, `just viewer-web-build`). Against the built package:

```sh
node dev/scripts/test-assets-worker.mjs    # no install needed
node dev/scripts/test-local-worker.mjs     # local read/error contracts, network forbidden
node dev/scripts/test-local-install.mjs    # folder mapping and validation
node dev/scripts/test-game-worker.mjs      # needs an install: game exports and CD movie reads
```
