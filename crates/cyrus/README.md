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
`--skip-startup` bypasses the splash and waits behind the title for required
menu/HUD preparation. New Game and the temporary Load fallback export the
installed starting world in a worker and enter after its dependencies are ready.

Windowed launches show the shared install picker when their explicit, remembered,
or discovered game-files location is unusable. Continue validates and remembers
the location in `cyrus-config/install.json`, shared with the viewer. Headless,
world-listing and export commands keep contextual command-line errors.

Flags: `--export-startup` exports startup, menu, starting world, movies and HUD
for the browser and exits; `--install PATH` (else `$REDGUARD_DIR`, then the remembered location, then default local/GOG folders), `--scene STEM|ID`,
`--headless`, `--skip-startup`, `--control ADDR` (control server, default `127.0.0.1:7117`).
Settings come from `cyrus-config/settings.toml`.
With a prebuilt archive, extract it and run `./cyrus --install /path/to/Redguard`
from a writable directory. On Windows the executable is `cyrus.exe`.
The Linux archive uses Ubuntu 24.04 system libraries.
In any windowed launch, press F1 to choose a world, set flycam speed, and
change runtime fidelity. The menu remains available across startup and world entry.

This crate owns native settings and preference storage, binding initialization,
scene preparation, debug world options and control-command application.
`cyrus-control::server` handles TCP framing. Headless assembly omits the camera
and audio backend; windowed assembly adds the shared presentation plugins.

Shared browser/CLI options use exactly the same names:

| CLI | Browser URL | Meaning |
| --- | --- | --- |
| `--install PATH` | `?install=URL` | Read-only original install, local directory or install-service URL |
| `--scene START` | `?scene=START` | Load a map stem or WORLD.INI ID directly |
| `--view=x,y,z,qx,qy,qz,qw` | `?view=x,y,z,qx,qy,qz,qw` | Initial scene-space inspection camera pose; requires `scene` and a unit quaternion |
| `--skip-startup` | `?skip-startup=true` | Bypass the splash |
| `--water modern` | `?water=modern` | Water fidelity mode |
| `--fog modern` | `?fog=modern` | Fog fidelity mode |
| `--sky modern` | `?sky=modern` | Sky fidelity mode |

Mode values are `faithful` or `modern`. Native flags override saved settings.
Native-only flags include headless/control and export commands. `--export-scene
--scene NAME` prepares one world and exits; `--list-worlds` prints accepted names
as JSON. The browser install service uses these existing asset paths without
starting a game window.

[cyrus-startup](../cyrus-startup/README.md) supplies splash, title, the book menu,
settings, cinematic playback and world-entry loading. Hosts add it alongside
cyrus-render and cyrus-audio.
