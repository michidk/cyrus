# cyrus-assets

Renderer-independent asset handling for the game and the asset viewer. It
validates the install, prepares original files into portable cache records and
generated media (GLB, PNG, WAV, JSON sidecars), and serves the viewer's catalog
and previews. [redguard-preservation](https://github.com/michidk/redguard-preservation)
(`rgpre`) owns parsing and recovered engine rules; this crate owns install
validation, catalog identities, cache records and generated files. Runtime
animation, collision queries, mesh bindings and audio handles belong to the
consuming crates; `cyrus-media` owns PCM conversion and the movie decoder, and
`cyrus-soup::program` owns SOUP program types.

Features: without features only the portable records compile. `parsers` adds
preparation through `rgpre` on native and browser targets; `native` adds
read-only install discovery, preload scans and the filesystem/disk-cache
adapters. `test-support` enables `parsers` and exposes `source::test_support::MemoryAssets` for integration
tests that need owned original-file bytes. It validates paths and manifests,
retains listing order, and rejects out-of-bounds reads. Specialized audit,
sparse-range or fault-injection sources can wrap it where appropriate.

## Modules

| Module | Responsibility |
| --- | --- |
| [install](src/install.rs) | `GameInstall::locate`/`resolve`/`open`: read-only install discovery and validation (`native`) |
| [source](src/source.rs) | `ReadAssets`, the read-only byte source shared by native files and the browser worker |
| [runtime](src/runtime.rs) | `runtime::Library`: viewer catalog, archive expansion and preview decoding |
| [native](src/native.rs) | Filesystem adapter and optional disk cache for the shared parsers |
| [game](src/game.rs) | `game::prepare`: cache-relative bytes for one requested game asset, on either target |
| [scene](src/scene.rs) | Native world export and installed world selection |
| [script_export](src/script_export.rs), [script](src/script.rs) | SOUP program descriptors and world script cache records |
| [environment](src/environment.rs), [player](src/player.rs), [collision](src/collision.rs), [animation](src/animation.rs) | World, player, collision and animation records; preparation lives in the matching `*_export` modules |
| [hud](src/hud.rs), [menu](src/menu.rs), [logbook](src/logbook.rs), [overlay](src/overlay.rs), [cursor](src/cursor.rs), [console](src/console.rs) | Presentation records for HUD, menus, logbook, script overlays, cursors and console |
| [sound](src/sound.rs), [music](src/music.rs), [movie](src/movie.rs), [startup](src/startup/mod.rs) | Effects, dialogue, CD music, cinematics and startup entry policy |
| [ini_patch](src/ini_patch.rs), [script_patch](src/script_patch.rs), [text_patch](src/text_patch.rs) | Mod-declared changes applied during preparation |
| [software_sky](src/software_sky.rs) | Software sky previews for the viewer |

`docs/MAP.md` lists every module. HUD font styles retain the installed colour scale
and independent main/shadow alpha; legacy alpha-only records retain full colour.

`framebuffer::blend` reconstructs stored destinations and blends encoded integer channels for loading fades and HUD composition.
`framebuffer::store` supplies the published ordered RGB565 encoded-channel storage to the logbook reader and HUD artwork. Coordinates are top-left framebuffer pixels.

## Usage

```rust
let install = cyrus_assets::install::GameInstall::locate(None)?;     // native
let files: std::collections::BTreeMap<String, Vec<u8>> =
    cyrus_assets::game::prepare(&source, "loading/START.loading")?;  // any ReadAssets
```

Native hosts write the returned files to the cache directory from
`cyrus-config`; the browser worker (`cyrus-assets-worker`) hands them to Bevy in
memory. The viewer's catalog behaviour is described in
[docs/ASSET_VIEWER.md](../../docs/ASSET_VIEWER.md).

## Boundaries

- The install is read-only. Exports go only to a caller-supplied cache
  directory, published by rename; generated paths are portable slash-separated
  cache keys and reject absolute paths and traversal.
- Changed export semantics must bump `CATALOG_VERSION` (in `src/lib.rs`) or the
  record's own schema version, so older caches are prepared again instead of
  being read with the wrong meaning.
- Missing or conflicting source data stays explicit (an `unsupported` entry or a
  contextual error), never an invented default.
- Native and browser preparation share one code path; do not add host-specific
  copies of world tables or settings. Settings come from installed INI files
  through `rgpre`.
- `GameInstall::open` also admits a software-only DOS install (`3dart`, `maps`,
  `RG.EXE`, no `fxart`) as `Edition::Software` and reads its `TEXTURE.###` banks;
  Glide wins when both art folders exist. Menu, HUD, logbook, effect and weapon
  preparation still need `fxart` (tracked on #91).
- Format or engine facts found here go to preservation's `docs/`, not into this
  README.

The book cache stores upload-ready palette and texture colours following the
[preserved startup palette contract](https://github.com/michidk/redguard-preservation/pull/343).
Generic COL parsing retains the file bytes.

Startup preparation stores one unlabelled `startup/title.png` and the installed
low font and palette-derived tints in `startup/labels.title`. Runtime presentation
paints version and status text; export removes the three legacy labelled variants.

## Testing

```sh
cargo test -p cyrus-assets                       # portable records
cargo test -p cyrus-assets --features native     # export and installed-data contracts
just verify assets
```

Installed-data tests use `REDGUARD_DIR` or the repository's test install link
and report a skip when neither is available; `tests/software_edition.rs` uses
`REDGUARD_SOFTWARE_DIR` for a software-only install. Exported content stays in the
ignored local cache.

Examples (all need `--features native` and an install; keep output under `magic/`):

- `soup_inventory` prints a JSON census of SOUP instructions and native calls
  across every world.
- `movie_window -- INSTALL OUTPUT [MOVIE_INDEX] [FRAME_COUNT] [FIRST_FRAME]`
  exports a decoded cinematic window for `dev/scripts/test-browser-movie-seek.mjs`.
- `software_sky -- INSTALL WORLD DRAW HEADING PITCH SCREEN_SCALE OUTPUT.png`
  renders one software sky frame.

The native `cyrus --audit-worlds` command reports skipped placements and texture
fallbacks for every world.

Declared BSI skies are prepared independently of terrain WLD availability.
The same environment record supplies them to native and browser rendering.
