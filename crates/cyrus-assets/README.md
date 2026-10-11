# cyrus-assets

Shared, renderer-independent asset handling for the game and asset viewer.
Preservation (`rgpre`) owns parsing and recovered engine rules. This crate owns
install validation, catalog identities, portable preview descriptions, generated
files, PCM conversion, and the portable movie decoder wrapper.

Native callers open an explicit `GameInstall`, then `native::Library`. Discovery
preserves files with duplicate names and gives archive members separate IDs.
`prepare` decodes one selected entry at runtime.
Unknown files and decoder failures remain inspectable catalog entries.
The library retains only the most recently used decoded source, so archive
members reuse parsing without retaining the entire install in memory.

The native exporter writes only to the supplied cache directory. A generation
hash includes the original-file manifest and schema version. Changed export semantics must bump `CATALOG_VERSION`.
Complete files are published by rename. Invalid preview manifests and missing
media are regenerated. Old generations remain recoverable and can be removed
manually when no client needs them.

Both targets use `runtime::Library` and its read-only `ReadAssets` interface.
Native adapters read files; the browser worker reads original bytes over HTTP.
Generated media is returned as bytes. Only native callers write a disk cache.
Browser presentation registers generated files with Bevy in memory.

Run contracts with `cargo test -p cyrus-assets -- --nocapture`. Installed-data
checks use `REDGUARD_DIR`, or the repository's test install link. They explicitly
skip when neither is available. No game data is included in tests.

The pinned preservation [browser world fixes](https://github.com/michidk/redguard-preservation/commit/054bbb06bbd0e1f142d6bad4d2be14bdadda611e)
keep terrain arrays off the WASM stack and avoid decoding unused archive models.
