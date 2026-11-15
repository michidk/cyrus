# cyrus-launcher

The window that starts cyrus: the original Redguard launcher, recreated, with its
background and buttons read at run time from the player's own `REDGUARD.EXE`
(plain buttons when that file is not found; nothing is shipped). **Play Redguard**
starts the `cyrus` executable beside the launcher and closes it. **Mods** switches
installed mods on or off, **Game Options** holds the game folder, the intro-movie
switch, the faithful or modern choice per presentation area and the cheats, and
**Technical Support** opens the issue tracker. The launcher does not link the game
and uses eframe with the glow backend, so wgpu stays out of the binary. Release
bundles ship it beside `cyrus` (see [setup](../../docs/SETUP.md#releases)).

## Modules

| Module | Responsibility |
| --- | --- |
| [main](src/main.rs) | The window, buttons and dialogs |
| [artwork](src/artwork.rs) | Reads the original launcher artwork from `REDGUARD.EXE` |
| [lettering](src/lettering.rs) | Labels without an original bitmap (Mods), assembled from the bitmaps' letters |
| [theme](src/theme.rs) | Colours and widgets matching the original leather and gold |
| [mods](src/mods.rs), [fidelity](src/fidelity.rs), [cheats](src/cheats.rs) | The mod, fidelity and cheat choices as the game's files hold them |
| [settings](src/settings.rs) | Saves only the configuration group the player changed |
| [launch](src/launch.rs) | The launcher's own choices and the command that starts the game |

## Files

The launcher reads and writes the files the game reads at startup, all in the
config directory chosen by `cyrus-config`:

| File | Holds |
| --- | --- |
| `mods.json` | The active mods; the folders in `mods/` are listed from their `mod.toml` |
| `settings.toml` | The `[fidelity]` table, one `faithful` or `modern` per area; other keys are kept |
| `install.json` | The game folder the game remembered, shown and used for the artwork |
| `REDGUARD.CHT` | The cheats; starts from the install's copy when there is no local one |
| `launcher.json` | The launcher's own choices (a picked game folder, skip the intro movie) |

## Notes

- Each control saves only its own group, so a group that failed to load is never
  overwritten; Play starts the game without rewriting configuration.
- Unreadable or malformed files (a mod manifest, a candidate cheat file) are
  reported and leave the previous selection intact. The Mods dialog lists enabled
  mods whose folders are missing so they can be disabled.
- The launcher keeps its own list of fidelity areas; a test against `cyrus-game`
  (a dev-dependency only) keeps it in step with the game's list.
- `artwork.rs` is the resource reader from the maiko launcher; it belongs in
  redguard-preservation once that crate reads launcher resources.

## Testing

```sh
cargo test -p cyrus-launcher
cargo build -p cyrus && cargo run -p cyrus-launcher   # Play needs the cyrus binary beside it
```
