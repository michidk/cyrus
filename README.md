<h1 align="center">cyrus</h1>

<p align="center">
  A faithful rebuild of <em>The Elder Scrolls Adventures: Redguard</em> in Rust and Bevy.<br>
  Bring your own GOG install. cyrus ships no game data.
</p>

<p align="center">
  <a href="#run-cyrus">Get started</a> ·
  <a href="https://michidk.github.io/cyrus/game/">Browser game</a> ·
  <a href="#asset-viewer">Asset viewer</a> ·
  <a href="docs/PLAYING.md">Playing guide</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

<p align="center">
  <a href="https://github.com/michidk/cyrus/releases/latest"><img src="https://img.shields.io/github/v/release/michidk/cyrus?color=green" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT license"></a>
  <a href="https://discord.com/invite/xjCCyqbxkp"><img src="https://img.shields.io/badge/Discord-Redguard%20community-5865F2?logo=discord&amp;logoColor=white" alt="Redguard community on Discord"></a>
</p>

<p align="center">
  <img src=".github/workflows/cyrus-banner.webp" alt="A green dragon watches an airship cross Redguard's island landscape" width="800">
</p>

> [!IMPORTANT]
> cyrus is early in development. You can start a new game, play in its worlds
> and save and resume, but a complete gameplay and fidelity playthrough has not
> been accepted.

## What works today

- Play the installed splash and title, then browse the data-driven book menu.
- Play unlocked movies, edit persistent settings, and start New Game with its
  introduction.
- Move, fight and interact in Redguard's worlds with the implemented controls.
- Save and load cyrus sessions through the book, with gameplay thumbnails, in
  the native and browser builds.
- Jump to any world and inspect it with a free camera (Backtick menu).
- Browse every asset in the separate asset viewer.

See [Playing and controls](docs/PLAYING.md) for controls, saving and current
limitations. See the [delivery issues](https://github.com/michidk/cyrus/issues/92) for remaining work and acceptance evidence.

Join the [Discord](https://discord.com/invite/xjCCyqbxkp) and discuss cyrus in
`#redguard-cyrus-rust`.

## Run cyrus

Buy and install or extract the GOG release of *The Elder Scrolls Adventures: Redguard*. Then:

1. Download the `cyrus` package for Windows, Linux or macOS from the [latest release](https://github.com/michidk/cyrus/releases/latest).
2. Extract the archive and start `cyrus.exe` on Windows or `cyrus` on Linux and macOS.
3. Choose the inner `Redguard` folder containing `RGFX.EXE`, `maps` and `fxart`. Keep the surrounding GOG folder intact so companion CD media remains available.
4. Choose **New Game** in the book menu. Use the in-game options to configure display, sound and controls.

See [Playing and controls](docs/PLAYING.md) for saving, resuming and key bindings. To build from source or select an install on the command line, see [Advanced use](#advanced-use).

## Web version

Open the [browser build](https://michidk.github.io/cyrus/game/) and click
**Choose game folder**. Select your main GOG folder, such as
`D:\Games\GOG Galaxy\Redguard`, containing both `DOSBOX` and the inner
`Redguard` folder. Files stay on your computer. Select the folder again after
reloading. The [browser guide](crates/cyrus-web/README.md) covers the bundled
game-file server, URL options and building from source.

## Videos

<table align="center">
  <tr>
    <th align="center"><a href="https://youtu.be/pG4tS6XbdIw">▶ Watch cyrus v0.5.0</a></th>
    <th align="center"><a href="https://youtu.be/B1s0oL_fFGU">▶ Watch the cyrus editor</a></th>
  </tr>
  <tr>
    <td align="center"><a href="https://youtu.be/pG4tS6XbdIw"><img src="https://i.ytimg.com/vi/pG4tS6XbdIw/mqdefault.jpg" width="320" alt="Watch cyrus v0.5.0 on YouTube"></a></td>
    <td align="center"><a href="https://youtu.be/B1s0oL_fFGU"><img src="https://i.ytimg.com/vi/B1s0oL_fFGU/mqdefault.jpg" width="320" alt="Watch the cyrus editor on YouTube"></a></td>
  </tr>
</table>

## Asset viewer

cyrus ships a separate asset viewer for Redguard's files: models, worlds,
textures, fonts, animations, scripts, audio and movies. Download a
prebuilt `cyrus-viewer` archive from the
[latest release](https://github.com/michidk/cyrus/releases/latest) or open the
[browser viewer](https://michidk.github.io/cyrus/viewer/) directly. See the
[viewer guide](crates/cyrus-viewer/README.md) for previews, controls, and browser
setup. The viewer uses a modern inspection interface and generic rendering.

| Model | World | Font |
| --- | --- | --- |
| ![Textured character model](dev/pages/viewer-model.webp) | ![Island world overview](dev/pages/viewer-world.webp) | ![Bitmap font specimen](dev/pages/viewer-font.webp) |

## Related projects

cyrus is two applications, the game and the asset viewer, and sits beside two other Redguard projects:

- [Redguard Preservation](https://github.com/michidk/redguard-preservation) ([docs](https://michidk.github.io/redguard-preservation/)): the file format, engine and configuration documentation cyrus is built from, and the `redguard-preservation` crate that parses the game's files.
- [Maiko](https://github.com/michidk/maiko) ([site](https://michidk.github.io/maiko/)): runs the original game natively on Windows, Linux and macOS, without DOSBox, by translating the original executable.

## Faithful by default

Faithful mode aims to reproduce what the player experiences: visuals, audio,
controls, camera behavior, script and animation timing, and game outcomes.
Modern mode is an opt-in setting that changes only graphics and sound; modern controls and camera
behavior come from the bundled `movement` mod.

Technical limits are not part of that goal. cyrus can use current hardware,
load faster, and run at a higher frame rate without changing game behavior.

File formats and engine documentation come from
[redguard-preservation](https://github.com/michidk/redguard-preservation).
cyrus publishes recovered format and engine facts through redguard-preservation.
No original code or game data is part of this repository.

## Acknowledgements

[RGUnity](https://github.com/RGUnity/redguard-unity) inspired cyrus and provided
foundational Redguard research used by this project. RGUnity is a human-developed
Unity reimplementation built without AI assistance. Its contributors focus on
meticulous fidelity and the small details that make the original game behave
correctly.

cyrus uses AI-assisted development to reach broad working coverage quickly. This
complements RGUnity's detail-first work. We are grateful to its contributors for
their research, implementation, and continued care for Redguard.

## Project documentation

- [How cyrus is built](docs/METHOD.md) explains how evidence and fidelity guide
  each change.
- [Setup](docs/SETUP.md) covers native, browser, and original-game runs.
- [Delivery issues](https://github.com/michidk/cyrus/issues/92) track remaining game support and acceptance.
- [Contributing](CONTRIBUTING.md) covers bug reports and pull requests.
- [Agent contract](AGENTS.md) defines the rules for coding agents.

Each crate also has its own `README.md`.

cyrus is written entirely by LLM coding agents (Codex and Claude Code), directed
and reviewed by the maintainer against the original game.

## Advanced use

Building from source requires Rust and `just`, plus the platform dependencies in the [setup guide](docs/SETUP.md). From the repository root:

```sh
just run --install /path/to/GOG/extracted/app/Redguard
```

Without `--install`, both native applications use `REDGUARD_DIR`, a remembered location, or the default local/GOG folders, including a `redguard` symlink in the working directory. If no usable install is found, the folder picker remembers your selection for both the game and viewer.

For the asset viewer, run `cargo run -p cyrus-viewer -- --install /path/to/Redguard`.

Use `just run --scene START` to open a world directly. The [setup guide](docs/SETUP.md) covers the browser build, the original game runner and all available commands.

cyrus stores settings, saves, caches and logs in `CYRUS_CONFIG_DIR` when set, otherwise in an existing `game-config/` folder in the working directory, or in your user profile's standard folders. On Linux these are `~/.config/cyrus`, `~/.cache/cyrus` and `~/.local/state/cyrus`.

## Name

cyrus is named after [Cyrus](https://en.uesp.net/wiki/Redguard:Cyrus), the main character of *Redguard*.

## License

cyrus is released under the [MIT license](LICENSE).

*cyrus is not affiliated with Bethesda Softworks.*
