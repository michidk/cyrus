# cyrus-app

Small launch and presentation helpers shared by the game (`cyrus`, `cyrus-web`)
and the viewer (`cyrus-viewer`): install selection, launch options, logging,
window and egui setup, the `cache://` asset source and a generic loading
overlay. It depends on Bevy and `cyrus-assets` (plus `cyrus-config` on native),
never on game or viewer plugins, and installs no player, menu, camera, network
or gameplay systems.

## Modules

| Module | Responsibility |
| --- | --- |
| [launch](src/launch.rs) | `InstallArgs` (`--install`, `REDGUARD_DIR`) with `locate()` and `select()`; `GameOptions` and `ViewerOptions`, shared by CLI flags and browser URL parameters (`from_query`) |
| [native_install](src/native_install.rs) | Install discovery order, the remembered `install.json` location and the in-app folder picker |
| [logging](src/logging.rs) | `LogArgs` (`--log`, `--log-file`, `--log-dir`): console plus daily rotating file log, `announce()`, `log_panics()` (native only) |
| [loading](src/loading.rs) | `LoadingOverlayPlugin` displaying a host-owned `LoadingProgress` |
| [lib](src/lib.rs) | `window()`, `EguiPlugin`, `register_cache()`, `browser_source()` |

## Usage

```rust
#[derive(clap::Parser)]
struct Args { #[command(flatten)] install: cyrus_app::launch::InstallArgs }

app.add_plugins(cyrus_app::EguiPlugin);       // before select()
args.install.select(&mut app);                // inserts GameInstall, or shows the picker
```

Install lookup order and the log flags are documented for players in
[docs/SETUP.md](../../docs/SETUP.md) (sections "Browser build" and "Logs").

## Notes

- `select()` shows the path field, Browse and Continue screen inside the
  application's existing egui context; it never opens another window or
  process. Continue validates and remembers the root; Cancel exits without
  changing saved preferences. Non-interactive callers use `locate()` and keep its errors.
- Browse uses `rfd`: the XDG desktop portal on Linux, native folder dialogs on
  Windows and macOS.
- `loading` owns only progress presentation; the caller owns progress,
  completion and state transitions. In the browser it updates the page's HTML
  overlay (pages set `data-loading="application"` to hand it over).
- `register_cache()` must run before Bevy's `AssetPlugin`. `EguiPlugin` pins
  `bevy_egui` and patches its WebGL shader for devices without binding arrays.
- `dev/scripts/check-core-boundaries.sh` builds an isolated consumer of this
  crate and rejects gameplay or presentation-crate dependencies.

## Testing

```sh
cargo test -p cyrus-app
just verify app
dev/scripts/check-core-boundaries.sh
```
