# cyrus-startup

Everything the player sees before and around gameplay: the 3dfx splash, the
title, the book menu and its settings, Save/Load pages, cinematics, world-entry
loading screens and fades, the logbook reader, the examine view, task GXA
overlays, the death "last game" dialog and the F1/F10 information and credits
screens. Native (`cyrus`) and browser (`cyrus-web`) hosts share it. It depends on
`cyrus-assets`, `-world`, `-game`, `-script`, `-text`, `-audio`, `-media`,
`-app` and Bevy, and must not depend on `cyrus-render`. Installed parsing and
the rules it implements belong to redguard-preservation.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `StartupPlugin`, `StartupPresentation`, `StartGame`, `WorldLoadRequest`, `QuitRequest` |
| [splash](src/splash.rs), [loading](src/loading.rs) | Live GPU 3dfx splash; title shown until menu/world assets are ready; world-entry loading screen |
| [preparation](src/preparation.rs) | Native `LoadingPlugin`: startup preload and headless parser-coverage scan |
| [menu](src/menu.rs) | Book menu data and state from `MENU.INI`: entries, layout, widgets, action IDs, `MenuContext` |
| [book_scene](src/book_scene.rs), [book_motion](src/book_motion.rs), [menu_raster](src/menu_raster.rs) | The book as a 3D scene, its authored turns and camera paths, CPU-painted page textures |
| [view3d](src/view3d.rs) | Shared lit material for installed model views (book, logbook scrolls, examine) |
| [settings](src/settings.rs) | Book preferences and key bindings applied to input and audio |
| [save_load](src/save_load.rs), [last_game](src/last_game.rs), [confirmation](src/confirmation.rs) | Slot pages and host storage requests, death dialog, mini-confirmation input and pixels |
| [movie](src/movie.rs), [movie_native](src/movie_native.rs), [movie_stream](src/movie_stream.rs), [movie_raster](src/movie_raster.rs) | Smacker playback: shared rules, native decoder, browser streaming, frame composition |
| [logbook](src/logbook.rs), [examine](src/examine.rs), [overlay](src/overlay.rs) | Logbook reader, examine view, `ViewGXA`/`AnimateGXA` overlays |
| [information](src/information.rs), [credits](src/credits.rs) | F1 version screen, F10 and ending credits |
| [fade](src/fade.rs), [gamepad](src/gamepad.rs) | World-entry fades; gamepad buttons mapped to keyboard keys |

## Usage

```rust
app.add_plugins(cyrus_startup::StartupPlugin);
// Skip the splash but keep required preparation:
app.insert_resource(cyrus_startup::StartupPresentation::without_splash());
// Native only: asset preparation and headless parser coverage.
app.add_plugins(cyrus_startup::LoadingPlugin);
```

- Loading fades publish their initial 12/256 step before consuming subsequent host intervals. At one nominal tick per update, all 22 steps are retained without single-precision accumulation skips; the installed minimum FPS caps a long update.
- Script and console `SceneRequest` reloads enter the installed loading artwork and fades before world preparation; their marker, wrapped heading and same-world reload semantics are retained. Ready caches complete the artwork fade-in before fade-out, with no simulated load delay.
- `StartGame` selects world-entry loading presentation; `WorldLoadRequest` enters
  a world (WORLD.INI ID or map stem) at marker zero through the same loading path.
- `MenuState` exposes menu status to control; `QuitRequest` asks the browser host
  to close its tab.
- The book render target uses the physical window extent on its first publication and follows later window resizes. The initial camera projection and display sprite fit that extent, and the camera and model frames use the current authored book motion pose.
- Hosts serve book storage requests through the `SaveLoadHost` and
  `LastGameLoader` resources; the session itself is captured and restored by
  [cyrus-session](../cyrus-session/README.md).
- `LogbookPlugin` and `OverlayPlugin` also work without `StartupPlugin`.

Launch flags (`--skip-startup`, `--scene`) and controls are in
[SETUP](../../docs/SETUP.md) and [PLAYING](../../docs/PLAYING.md). The shipped
title artwork is an accepted difference, see
[DIFFERENCES](../../docs/DIFFERENCES.md#title-artwork).

## Notes

- Menu content comes from `MENU.INI` through rgpre, following preservation's
  [book-menu rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md).
  Do not duplicate entries, labels or action IDs in Rust.
- The book, logbook scrolls and examine view are real meshes under a perspective
  camera. Texture filtering and RGB565 dithering follow preservation's
  [sky](https://github.com/michidk/redguard-preservation/blob/v0.12.5/docs/engine/sky.md) and
  [fog](https://github.com/michidk/redguard-preservation/blob/v0.12.5/docs/engine/fog.md#integer-precision-and-framebuffer-dithering)
  rules; dialogs keep their original 4:3 coordinates.
- Examine model caches belong to their gameplay panel; leaving and re-entering gameplay republishes the model even when its item and pose are unchanged.
- Examine scenes refresh when the loaded world lighting or palette changes, even at an unchanged item pose, following preservation's [examine view rules](https://github.com/michidk/redguard-preservation/blob/v0.12.5/docs/engine/hud.md#examine-view).
- The reader stores paper, lettering and scroll fragments with the published ordered RGB565 framebuffer precision before presentation.
- The standalone logbook retries scroll publication when 3D asset stores arrive after its paper; a cached paper image alone does not complete the reader scene.
- The portable menu cache (`cache/menu/book.menu`) carries a schema version;
  changing what it stores means bumping it so older exports are rejected.
- Preferences persist before they apply; saving one setting leaves unedited
  installed Sound and Music levels alone. Keep the audio order `ApplyPreferences -> AnimationSystems -> PlayCues`.
- Movie playback is bounded and streaming on both hosts: the audio clock drives
  the picture, and menu return or seek cancels pending work. Rules follow
  preservation's `engine/movies.md`.
- World-entry loading follows preservation's
  [startup rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/startup.md#world-loading-screen);
  a failed artwork or cache request leaves the current player intact and can be retried.

## Testing

```sh
cargo test -p cyrus-startup
just verify startup
```

Tests that need the install print `skipped: no install` without one. Runtime
changes also need native or browser playtests with an explicit install
(see [SETUP](../../docs/SETUP.md)).

Script checks for the browser movie path and book rendering, in `dev/scripts/`:

| Script | Checks |
| --- | --- |
| `node test-movie-stream.mjs` | Movie streaming, seeking inside queued PCM, cancelled completions |
| `node test-browser-movie-clock.mjs CDP_URL PAGE_URL REFERENCE_WAV RESULT_JSON` | Continuation timing and PCM against a real WebAudio clock |
| `node test-browser-movie-seek.mjs CDP_URL PAGE_URL WINDOW_URL RESULT_JSON` | Pause/seek with queued audio; the window comes from the assets `movie_window` example |
| `check-book-filter.py prepare` / `compare` / `sample-grid` | Four-bit book texture filtering, via disposable `book.menu` caches |
| `check-book-framebuffer.py prepare` / `check` | Ordered-dither RGB565 output on a constant-colour cache |

The model material uses 16-byte uniform bindings so the book, logbook and examine
views work on WebGL devices that require aligned uniform blocks.

The Python checkers need Pillow and have their own tests
(`test-check-book-filter.py`, `test-check-book-framebuffer.py`). They replace
the menu cache, never the install; restore the original cache afterwards.

The reader paper reduces nonzero expanded palette channels by one before storage, following the original raster comparison documented in [preservation PR #352](https://github.com/michidk/redguard-preservation/pull/352). This presentation rule uses the published parser unchanged.

Logbook scrolls use the published software-Voodoo four-bit encoded-channel filter before lighting and RGB565 storage. `dev/scripts/check-book-filter.py sample-grid --framebuffer` checks a full-brightness stripe fixture through the production reader screenshot; it rejects continuous filtering while admitting stored integer-filter samples.
