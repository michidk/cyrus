# cyrus (binary)

The game executable. Parses arguments, finds the install, assembles the Bevy
app from `cyrus-game` plugins, and runs it.

```sh
cargo run -p cyrus -- --scene START         # window
cargo run -p cyrus -- --headless --scene 27 # preload, scene export and control server only
cargo run -p cyrus -- --export-all-worlds   # write every WORLD.INI world to cyrus-config/cache
```

Without `--scene`, a windowed launch runs startup through the book menu.
`--scene` and headless launches retain direct scene loading.
`--skip-startup` opens the book menu without exporting splash/title assets; it still exports the installed menu.

Flags: `--export-startup` renders startup images and exports the book-menu cache and exits; `--install PATH` (else `$REDGUARD_DIR`, else `./redguard`), `--scene STEM|ID`,
`--headless`, `--skip-startup`, `--control ADDR` (control server, default `127.0.0.1:7117`).
Settings come from `cyrus-config/settings.toml`.
With `--scene`, press F1 to choose a world, set flycam speed, and change runtime fidelity.

This crate holds no game logic; it only wires plugins. Window-dependent
plugins are added here so `cyrus-game` stays free of them.
