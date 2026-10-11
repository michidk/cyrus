# cyrus (binary)

The game executable. Parses arguments, finds the install, assembles the Bevy
app from assets, world, game and presentation plugins, and runs it.

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
With a prebuilt archive, extract it and run `./cyrus --install /path/to/Redguard`
from a writable directory. On Windows the executable is `cyrus.exe`.
The Linux archive uses Ubuntu 24.04 system libraries.
With `--scene`, press F1 to choose a world, set flycam speed, and change runtime fidelity.

This crate owns native settings and preference storage, binding initialization,
scene preparation, debug world options and control-command application.
`cyrus-control::server` handles TCP framing. Headless assembly omits the camera
and audio backend; windowed assembly adds the shared presentation plugins.
