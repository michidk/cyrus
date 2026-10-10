# cyrus

*The Elder Scrolls Adventures: Redguard* (1998), rebuilt in Rust and Bevy.
Point it at your own GOG install and it loads the island, the models, the
scripts and the sounds from there. No game data is bundled or distributed.

Status: startup presentation and a world viewer. Default launch shows the
original splash and title, then stops at the book menu. Nothing is playable yet.

- Buy the game on GOG, extract it, link `redguard` to its `Redguard/` folder.
- `just run` for startup, `just run --skip-startup` for the book menu,
  or `just run --scene START` for a world. See `docs/SETUP.md`.
- Faithful by default; modern controls, camera and graphics are opt-in
  switches in `cyrus-config/settings.toml`.

"Faithful" means the player experiences the same game: what you see and
hear, how the controls respond, the camera, the timing of scripts and
animations, and how every quest plays out. It does not mean copying the
original's limitations. cyrus loads quickly, runs smoothly and uses
current hardware well; slow loading, low frame rates and other technical
constraints are not part of what gets reproduced.

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
