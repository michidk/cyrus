# cyrus-assets

Validated portable animation, player, collision, environment, menu, HUD, movie and
sound cache records. Data-only lookup and validation live with their serialized
records. Runtime animation, collision queries, mesh bindings and audio handles
belong to the consuming crates.

The opt-in `native` feature enables read-only install discovery, preload scans,
native filesystem and disk-cache adapters. The `parsers` feature enables shared
scene/startup/menu/HUD/movie and sound/CD/voice/font preparation through `rgpre`
on native and browser targets. `game::prepare` returns cache-relative bytes from
a `ReadAssets` source; the browser worker requires no server-side exporter. Export paths are
caller-supplied; the install is never written. Sidecar filenames are stable; schema checks reject incompatible cache records. Native effects and exported audio are WAV bytes, never a Bevy audio handle.
Native movie preparation retains a read-only file or CD-image location and reads
only metadata. Its bounded reader closes the CD image when dropped. Browser movie
preparation loads metadata only; `cyrus_media::MovieStream` consumes compressed
chunks and discards decoded packets. The vendored `smk` codec adds sequential
packet input without replacing the upstream decoder.

Run `cargo test -p cyrus-assets` for portable records, or
`cargo test -p cyrus-assets --features native -- --nocapture` for export contracts.
Installed tests report skips when the install is unavailable. Exported content
stays in the ignored local cache; no game data ships in this crate.

The installed `every_world_exports_valid_animation_geometry` test exports every
WORLD.INI entry, including shared-map palette variants, and validates all exported
animation models and frames, including each authored ShowFrame reference through
its actor's RAAN table or each type-one placement's model frame table. Definitions
without a supported instance frame source are reported separately. It fails on
unsupported model references. This checks
asset coverage, not automatic animation playback or script progression.

World exports include `.animations` sidecars containing actor animation groups,
RAAN model references, and decoded model frame positions and normals. Missing or
unsupported model references remain explicit in the asset's `unsupported` map.
Original animation payloads and the RASC, RAST, RASB and RAVA script inputs are
retained too. Preservation supplies typed SOUP decoding; cyrus exports version-2 portable
program descriptors with names from the installed SOUP386.DEF. The independent
version-6 `.scripts` sidecar retains the validated SYSTEM.INI clock inputs, the installed actor-pool capacity, actor RASC
origins, definitions, fresh-game flag values, locals, strings,
initialized attributes, hooks and explicit unsupported records. Placement records retain
the original model name and complete model-mode byte for actor initialization.
Actor-load defaults come from preservation [PR #59](https://github.com/michidk/redguard-preservation/pull/59)
and installed SYSTEM.INI; model-path message-group assignment remains separate. Its content identity rejects
changed metadata. Animation metadata retains the same script identity; preparation
rejects animation/script sidecars from different exports. The animation inspection adapter also retains those same
program descriptors in animation metadata.
Embedded ROB models are retained when matching archive payloads agree; conflicting
payloads remain unsupported rather than selecting an archive without evidence.
Native and browser scene loading retain this data. Re-export worlds for schema 2
and the GLB source vertex/face mapping. Older sidecars are rejected with a
re-export error. In `cyrus-render`, explicit model poses update private meshes, normals, and bounds;
removing a pose or changing worlds restores the original mesh.

World export also prepares CYRUS's standalone RAAN frame-source models under
`<world>.player-models/`, with the world's palette, textures and source geometry
mappings. The `.player` sidecar retains their authored model-header radii in map
units for collision candidate selection. Missing standalone player models report
contextual export errors.
Re-export worlds before using explicit player model poses.

## Asset viewer

The `parsers` feature enables runtime parsing on native and browser targets; `native` includes it.

Shared, renderer-independent asset handling for the game and asset viewer.
Preservation (`rgpre`), pinned to the [byte-range revision](https://github.com/michidk/redguard-preservation/compare/main...feat/cd-member-ranges), owns parsing and recovered
engine rules. This crate owns install validation, catalog identities, portable
preview descriptions, generated
files. `cyrus-media` owns PCM conversion and the portable movie decoder wrappers.

Native game and viewer share `GameInstall::locate`, which resolves selected
library/package folders and install descendants through `GameInstall::resolve`.
Internal callers use `GameInstall::open` to validate an exact root, then
`native::Library`. Discovery preserves files with duplicate names and lists archives without reading them.
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

Run viewer contracts with `cargo test -p cyrus-assets --features native -- --nocapture`. Installed-data
checks use `REDGUARD_DIR`, or the repository's test install link. They explicitly
skip when neither is available. No game data is included in tests.

The pinned preservation [runtime loading fixes](https://github.com/michidk/redguard-preservation/commit/8c7a3f0f2866976d2a5885706b3fb7f007f02697)
keep terrain arrays off the WASM stack, support archive index reads, and expose
texture bank resolution so models load only their referenced banks.

The pinned [model frame snapshot fix](https://github.com/michidk/redguard-preservation/pull/51)
exposes compressed and full-precision animation frames through the shared parser API.
The static SOUP census visits every WORLD.INI entry separately, including shared
maps with different palettes, and every decoded actor definition, including
unplaced standalone actors. It counts instructions, nested native calls, operand
and reference forms, hooks, and unsupported records. Function declarations remain
separate from observed call sites. Counts describe authored occurrences, not
reachability, runtime support, exercised operations, or original-game parity.
World preparation failures remain in the report and cause a failing exit status.

```sh
mkdir -p magic/artifacts/soup-inventory
cargo run -p cyrus-assets --features native --example soup_inventory > magic/artifacts/soup-inventory/census.json
```

Keep this generated installed-content report under `magic/`. Handler ownership,
missing services, and scenario evidence belong to the completion investigation;
the asset census does not infer them from function names.

Placed-object frame lookup uses the instance metadata retained by preservation,
following [the frame-source rules](https://github.com/michidk/redguard-preservation/blob/6488b6e/docs/engine/animation.md#placed-object-frame-sources).
This lookup does not establish automatic actor or animation eligibility.
