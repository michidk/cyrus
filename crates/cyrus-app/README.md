# cyrus-app

Small presentation and launch helpers shared by the game and viewer.

- `launch::InstallArgs` defines `--install` and `REDGUARD_DIR`; `locate()` shares
  native default-folder discovery and resolution of GOG library/package folders
  and install subdirectories. It checks explicit/environment paths before
  `cyrus-config/install.json`, then conventional folders. `locate_or_prompt()`
  opens the shared native path field, folder picker and Continue screen on failure.
  Continue validates and saves the resolved install root, closes the launcher
  window, and starts the same executable with its launch options preserved.
  Cancel leaves preferences untouched. Noninteractive callers retain `locate()`
  errors. Linux Browse uses a desktop file portal or Zenity; Windows and macOS
  use native folder dialogs. `GameOptions` and `ViewerOptions` define the
  launch names and validation used by both CLI and URL parameters.
- `browser_source` reads the validated install/cache URLs from the shared bootstrap.
- `register_cache` registers the shared `cache://` asset source before Bevy's
  `AssetPlugin` starts.
- `window` creates the common native/browser window configuration.
- `EguiPlugin` installs the pinned adapter and its WebGL shader adjustment.
- `LoadingOverlayPlugin` displays `LoadingProgress`. The caller owns progress,
  completion, and application state transitions. Native builds draw a Bevy
  overlay. Browser builds update the existing HTML overlay; pages using that
  handoff set `data-loading="application"` so the download loader retains it.

This crate depends on Bevy and `cyrus-assets`, never on game or viewer plugins.
It does not install player, menu, world-camera, network, or gameplay systems.
