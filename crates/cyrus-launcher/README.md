# cyrus-launcher

The window that starts cyrus. It shows the original Redguard launcher, recreated, with its
background and buttons read at run time from the player's own `REDGUARD.EXE` (plain buttons when
that file is not found; nothing is shipped). **Game Options** holds the game files, the mods and the
faithful or modern choice for each presentation area; **Play Redguard** starts the `cyrus`
executable beside the launcher and closes the launcher.

The launcher does not link the game. It reads and writes the files the game reads at startup:

| File | Holds |
| --- | --- |
| `mods.json` | the active mods; the folders in `mods/` are listed from their `mod.toml` |
| `settings.toml` | the `[fidelity]` table, one `faithful` or `modern` per area; other keys are kept |
| `install.json` | the game folder the game remembered, shown and used for the artwork |
| `launcher.json` | the launcher's own choices (a picked game folder, skip the intro movie) |

All of them live in the config directory chosen by `cyrus-config`. A test pins the launcher's list
of fidelity areas to the game's.

```text
cargo run -p cyrus-launcher        # needs target/debug/cyrus beside it (cargo build -p cyrus)
cargo test -p cyrus-launcher
```

`src/artwork.rs` is the resource reader from the maiko launcher; it belongs in
redguard-preservation once that crate reads launcher resources.
