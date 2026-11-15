# cyrus-launcher

The window that starts cyrus. It shows the original Redguard launcher, recreated, with its
background and buttons read at run time from the player's own `REDGUARD.EXE` (plain buttons when
that file is not found; nothing is shipped). **Play Redguard** starts the `cyrus` executable beside the launcher and closes the launcher.
**Mods** lists the installed mods and switches them on or off. **Game Options** holds the game
files, the intro-movie switch, the faithful or modern choice for each presentation area, and the
cheats. **Technical Support** opens the issue tracker. The Mods label is assembled from letters of
the original bitmaps; the Cancel button keeps its original bitmap, because the artwork has no Q for
a Quit label.

The Mods dialog also lists enabled mods whose folders are missing or whose manifests cannot be
read, with a Disable action. Opening the dialog keeps those selections until the player changes
them, including when no readable mods remain installed.

The launcher does not link the game. It reads and writes the files the game reads at startup:

| File | Holds |
| --- | --- |
| `mods.json` | the active mods; the folders in `mods/` are listed from their `mod.toml` |
| `settings.toml` | the `[fidelity]` table, one `faithful` or `modern` per area; other keys are kept |
| `install.json` | the game folder the game remembered, shown and used for the artwork |
| `REDGUARD.CHT` | the cheats; the launcher writes the config directory's copy and starts from the install's when there is none |
| `launcher.json` | the launcher's own choices (a picked game folder, skip the intro movie) |

Changing the selected game folder refreshes the displayed cheats from the same source the game
will read. A local cheat file still takes precedence. An unreadable or malformed candidate cheat
file leaves the previous selection and displayed state intact and reports the error.

All of them live in the config directory chosen by `cyrus-config`. A test pins the launcher's list
of fidelity areas to the game's.

```text
cargo run -p cyrus-launcher        # needs target/debug/cyrus beside it (cargo build -p cyrus)
cargo test -p cyrus-launcher
```

`src/artwork.rs` is the resource reader from the maiko launcher; it belongs in
redguard-preservation once that crate reads launcher resources.
