# cyrus (binary)

The game executable. Parses arguments, finds the install, assembles the Bevy
app from `cyrus-game` plugins, and runs it.

```sh
cargo run -p cyrus -- --scene START         # window
cargo run -p cyrus -- --headless --scene 27 # preload, scene export and control server only
cargo run -p cyrus -- --export-all-worlds   # write every WORLD.INI world to cyrus-config/cache
```

Flags: `--install PATH` (else `$REDGUARD_DIR`, else `./redguard`), `--scene STEM|ID`,
`--headless`, `--control ADDR` (control server, default `127.0.0.1:7117`).
Settings come from `cyrus-config/settings.toml`.
Press F1 in the native game to choose a world, set flycam speed, and change runtime fidelity.

This crate holds no game logic; it only wires plugins. Window-dependent
plugins are added here so `cyrus-game` stays free of them.
