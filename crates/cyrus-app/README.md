# cyrus-app

Small presentation and launch helpers shared by the game and viewer.

- `launch::InstallArgs` defines `--install` and `REDGUARD_DIR`; `locate()` shares
  native default-folder discovery and resolution of GOG library/package folders
  and install subdirectories. It checks explicit/environment paths before
  `cyrus-config/install.json`, then conventional folders. `select()` inserts the
  install or displays the shared path field, Browse and Continue screen in the
  application's existing egui context. Continue validates and saves the resolved
  root, inserts `GameInstall`, and removes the picker. It never opens another
  application window or starts another process. Hosts wait for the install before
  preparing assets; the game prepares its initial images in a background task.
  Cancel leaves preferences untouched. Noninteractive callers retain `locate()`
  errors. Linux Browse uses a desktop file portal or Zenity; Windows and macOS
  use native folder dialogs. `GameOptions` and `ViewerOptions` define the
  launch names and validation used by both CLI and URL parameters.
  The game's optional `view` inspection pose also accepts menu entry without
  `scene`; the rendering host applies it when the first world is framed.
- `browser_source` reads the validated install/cache URLs from the shared bootstrap.
- `register_cache` registers the shared `cache://` asset source before Bevy's
  `AssetPlugin` starts.
- `window` creates the common native/browser window configuration.
- `EguiPlugin` installs the pinned adapter and its WebGL shader adjustment.
- `loading::LoadingOverlayPlugin` displays `loading::LoadingProgress`. The caller
  owns progress, completion, and application state transitions. Native builds draw a Bevy
  overlay. Browser builds update the existing HTML overlay; pages using that
  handoff set `data-loading="application"` so the download loader retains it.

This crate depends on Bevy and `cyrus-assets`, never on game or viewer plugins.
It does not install player, menu, world-camera, network, or gameplay systems.

`loading` owns only generic progress presentation. Game settings, fidelity,
audio listener selection and speech/subtitle integration belong to their game
or host modules. The isolated app consumer in
`dev/scripts/check-core-boundaries.sh` rejects gameplay and presentation-crate
dependencies, even when the full workspace enables their features.
