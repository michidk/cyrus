# cyrus

*The Elder Scrolls Adventures: Redguard* (1998), rebuilt in Rust and Bevy.
Point it at your own GOG install and it loads the island, the models, the
scripts and the sounds from there. No game data is bundled or distributed.

Status: early scaffold. It reads the install, exports a scene and shows it.
Nothing is playable yet.

- Buy the game on GOG, extract it, link `redguard` to its `Redguard/` folder.
- `just run` — see `docs/SETUP.md`.
- Faithful by default; modern controls, camera and graphics are opt-in
  switches in `cyrus-config/settings.toml`.

File formats and engine documentation come from
[redguard-preservation](https://github.com/michidk/redguard-preservation).
Related projects: [RGUnity](https://github.com/RGUnity/redguard-unity) (Unity).
cyrus is informed by reverse engineering of the original game; recovered
format and engine facts are published in redguard-preservation, and no
original code or data is part of this repository.

Documentation: `docs/METHOD.md` (how changes are judged), `docs/SETUP.md`
(running it), `docs/ROADMAP.md` (status), `CONTRIBUTING.md` (bug reports and
pull requests), `AGENTS.md` (rules for every change and for coding agents),
and a `README.md` in every crate.

Not affiliated with Bethesda.
