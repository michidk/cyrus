# WORKFLOW — how the maintainer's agents work

This is the maintainer's own setup: direct commits, roadmap ownership, the
untracked `magic/` working memory and how agents hand work to each other.
Outside contributors do not need it; `CONTRIBUTING.md` is their path. Rules
that bind everyone are in `AGENTS.md`.

## Glossary

- **install** — the player's own GOG Redguard, linked as `redguard`. Read-only.
- **magic/** — untracked working memory: installer, extracted game, reference
  clones, research, journals, task artifacts. Its `README.md` and
  `METHODOLOGY.md` are local and describe the research side.
- **artifact** — one folder per task under `magic/artifacts/`. Knowledge that
  is not in an artifact, the roadmap or the tracked docs does not exist for
  the next agent; chat history is not memory.
- **slice** — one piece of work by one agent in one sitting.
- **evidence** — something the next agent can rerun: a test, a command and its
  output, a scene + position + inputs, a screenshot pair. Numbers copied from
  elsewhere are not evidence.
- **reference** — a clone under `magic/reference/`. A research hint, never
  proof of what the original does.

## Starting and landing work

- `just setup` once per clone: the publish-check pre-commit hook and the
  untracked agent config for both agents (`.claude/` skills link and MCP
  pre-approval, `.codex/config.toml` MCP server). Codex and Claude Code read
  the same `AGENTS.md` (`CLAUDE.md` links to it), skills and MCP tools; nothing
  here works for one and not the other.
- No `cargo` and no sudo (the agent containers): `just toolchain` installs
  rustup, the wasm target, wasm-bindgen and Bevy's Linux libraries under
  `magic/build-tools/`; every `just` recipe and `gates.sh` pick it up. CI
  uses the system toolchain and runs the same gates plus a wasm32 check.
- `git pull` before starting and while working. Commit completed, verified
  work directly to `main` and push; no feature branches or pull requests
  unless asked. Other agents work in the same checkout: stage only the files
  your task touched.
- Claim in `docs/ROADMAP.md` when starting (one row: items, agent/session,
  artifact folder) and remove the row when finished. Progress, blockers and
  handoff go in the task artifact, not the roadmap; the roadmap changes only
  when something is done, with verification evidence. Update resource coverage
  when loading or runtime support changes; distinguish parsed data from
  working behavior. Preserve other agents' rows.
- One worktree per agent. Several agents in one checkout leave each other's
  half-done files in the tree and fight over the cargo build lock;
  `t3_thread_launch` with a `workspaceStrategy`, or `git worktree add`, gives
  each task its own.
- Stop what you started (`just stop`, containers, previews).

## Artifacts

```
magic/artifacts/2026-10-10-water/
  README.md                     # status header, see below
  1-surface-palette-FINAL.md
  2-wave-timing-PART.md
  3-compare-shots-READY/        # a slice with extra files is a folder with a README
    README.md
    cyrus.png  original.png
```

Each slice is one file `<N>-<name>-<SUFFIX>`. The suffix says whose move it is:

- `-FINAL` — the slice's goal is done.
- `-PART` — unfinished; another agent continues from here.
- `-READY` — the human must act: playtest, decide, supply something.

When unsure, choose the suffix that keeps responsibility with you. Past slices
are never edited or renamed; a correction is a new `N+1` slice. Only the
artifact `README.md` and living docs change. Logs, probes and JSON dumps go in
the artifact, not loose in `magic/`.

The artifact `README.md` starts with:

```
---
status: active | blocked | landed
updated: 2026-10-10
owner: <agent/session>
next: <one line: what the next agent or the human does>
---
<one-line summary, at most 200 characters> Current slice: 2-wave-timing-PART.md
```

## Handing work over

The final report, in chat and in the last slice, states:

- agent and model (the exact model id, or "not recorded"; never guessed);
- what was verified and how (the evidence, rerunnable);
- what was **not** done or not checked;
- whether the change fixed the root cause or only the symptom;
- which probes or scaffolding were removed, and which were kept and why.

Every claim about the game carries one label: **Verified** (read from a game
file, seen in the original, or in a preservation doc), **Inferred** (follows
from such facts), **Unknown**. Unlabelled claims are Unknown. The same labels
go on anything sent to redguard-preservation.

Before committing, read the diff: remove probe code and debug leftovers, keep
only tests that guard a contract. `just gates` runs `publish-check`; passing it
means the forbidden shapes are absent, not that the change is clean.

## Reference projects

`just reference` fetches the reference clones into `magic/reference/` (run it
again to update).

- `redguard-preservation` — `docs/formats`, `docs/engine`, `docs/config` and
  the `rgpre` parsers. Read the format documentation before writing a loader;
  it is also [published online](https://michidk.github.io/redguard-preservation/).
- [redguard-unity](https://github.com/RGUnity/redguard-unity) — code under
  `Assets/Scripts/` (`Core`, `FFI`, `GameTest`), consumes `rgpre` through FFI.
  Use its names and behavior as research hints; never copy its code. The
  roadmap records the source review's limitations.

References serve roadmap steps 0–3 (world playback, player controller,
interaction and persistence); they do not create a separate feature-parity goal.

## Playtesting

The `cyrus-playtest` skill is the loop: start cyrus (`just run`,
`just run-native`, `just run-headless`), drive it through the MCP tools or the
control port on `127.0.0.1:7117`, compare with the original under `just retro`
(preview `cyrus-retro`), and put the comparison in the commit message.

A comparison is evidence when it reruns: `just replay <fixture> <label>` plays
a `dev/fixtures/*.jsonl` fixture (scene, inputs, waits) over the control port
and captures both games. A capture copied to
`cyrus-config/compare/baseline/<label>-cyrus.png` makes the next replay a
regression check with a tolerance. `just smoke-mcp` fails when a run modified
the install.

## Traps already paid for

- Several agents in one checkout: `cargo` blocks for the whole of another
  session's build ("Blocking waiting for file lock"). `just mcp` uses its own
  target dir; everything else needs its own worktree.
- Rust tests capture `eprintln!`; "skipped: no install" only shows with
  `--nocapture`, which is why the gates pass it.
- DOSBox-X's own capture key never fires under Xvfb without a window manager;
  the retro side is grabbed with `scrot`.
- Bevy resolves relative asset paths against the executable directory when
  `CARGO_MANIFEST_DIR` is unset; the dev container sets `BEVY_ASSET_ROOT`.
- The hosted CI runner runs out of disk on the debug build and the linker dies
  with a bus error; `ci.yml` frees the preinstalled SDKs first.
- The managed `gh` refuses repositories the GitHub App is not installed on;
  use plain `curl` against the API for public lookups.
- `cargo install` outside the repo needs `RUSTUP_TOOLCHAIN=stable`; the
  `rust-toolchain.toml` only applies inside it.
