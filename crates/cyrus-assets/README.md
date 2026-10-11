# cyrus-assets

Shared, renderer-independent asset handling for the game and asset viewer.
Preservation (`rgpre`) owns parsing and recovered engine rules. This crate owns
install validation, catalog identities, portable preview descriptions, generated
files, PCM conversion, and the portable movie decoder wrapper.

Native callers open an explicit `GameInstall`, then `native::Library`. Discovery
preserves files with duplicate names and lists archives without reading them.
`expand` reads a selected archive index and gives members separate IDs; `prepare`
decodes one selected member from its byte range at runtime. Model previews accept
an optional palette from their world references; absence selects the first known
palette, while an explicitly empty selection uses embedded colors or grayscale
indices. Model frame poses come from preservation's shared conversion API. Palette choices have separate cache identities.
Unknown files and decoder failures remain inspectable catalog entries.
The library retains the most recently used source or archive index, so members
reuse indexing without retaining all payloads in memory.

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

The pinned preservation [runtime loading fixes](https://github.com/michidk/redguard-preservation/commit/8c7a3f0f2866976d2a5885706b3fb7f007f02697)
keep terrain arrays off the WASM stack, support archive index reads, and expose
texture bank resolution so models load only their referenced banks.

The pinned [model frame snapshot fix](https://github.com/michidk/redguard-preservation/pull/51)
exposes compressed and full-precision animation frames through the shared parser API.
