# Contributing

cyrus is a personal project, public so people can read it, build it and fix
it. The code is written entirely by LLM coding agents under the maintainer's
direction; decisions are the maintainer's, and no fix date is promised. The original
game is the specification: a change that makes cyrus behave more like it beats
a feature it never had.

## Bug reports

Welcome. What the root `README.md` claims to work outweighs anything it does
not promise. Include:

- the cyrus commit;
- OS and GPU, or the browser build and browser;
- that the install is the GOG release, and any changes made to it;
- the world (WORLD.INI entry or scene name) and the position;
- non-default fidelity modes from `settings.toml` from the config directory (`cyrus-config/` in a checkout);
- steps to reproduce and what happened;
- if you can, what the original game does in the same place.

Never attach files from your install, extracted assets or save games.

## Changes

- **Small and self-contained** (a crash, a wrong constant, a doc fix): open a
  pull request.
- **Structural** (a new crate, a new fidelity area, data flow between crates):
  open an issue first. Large branches that arrive unannounced will likely be
  declined, however good they are.
- Every behaviour change says where it comes from: a game file read through
  `rgpre`, a reproducible observation of the original, or a documented engine
  fact (see `docs/METHOD.md`). Do not guess constants.
- File-format and engine discoveries belong in
  [redguard-preservation](https://github.com/michidk/redguard-preservation);
  cyrus consumes them from there.
- Before submitting: `just gates` (publish-check, fmt, clippy, tests). Pull
  request titles follow Conventional Commits, e.g. `fix(player): …`.

`AGENTS.md` holds the rules every change follows. `docs/WORKFLOW.md` and the
untracked `magic/` folder describe the maintainer's own agent setup; you do
not need them.

## Game data

cyrus reads each player's own copy, bought on GOG, and distributes nothing
from it. Do not attach, commit or link game files, installers or keys in
issues, pull requests or releases.
