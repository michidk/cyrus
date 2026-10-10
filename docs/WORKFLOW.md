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
  untracked agent config (`.claude/`, `.codex/`: skills link, MCP pre-approval,
  Codex server).
- `git pull` before starting and while working. Commit completed, verified
  work directly to `main` and push; no feature branches or pull requests
  unless asked. Other agents work in the same checkout: stage only the files
  your task touched.
- Keep `docs/ROADMAP.md` current throughout the task: claim the relevant items
  with your agent/session name when starting, update progress and blockers at
  milestones, and release or hand off ownership when finished. Update resource
  coverage when loading or runtime support changes. Mark items done only with
  verification evidence; distinguish parsed data from working behavior.
  Preserve other agents' entries and include roadmap updates with the
  relevant change.
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

- what was verified and how (the evidence, rerunnable);
- what was **not** done or not checked;
- whether the change fixed the root cause or only the symptom;
- which probes or scaffolding were removed, and which were kept and why.

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
(preview `cyrus-retro`) or `just compare <label>`, and put the comparison in
the commit message.
