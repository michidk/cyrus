# cyrus — agent contract

cyrus is a faithful rebuild of *The Elder Scrolls Adventures: Redguard* (1998)
in Rust and Bevy. It reads the player's own GOG install at runtime and ships no
game data. The original game, run from that install, is the specification.
Redguard's file formats and engine facts are owned by the sibling project
[redguard-preservation](https://github.com/michidk/redguard-preservation)
(crate `rgpre`); cyrus consumes that crate and sends format discoveries back there.

Read in this order: this file, `docs/METHOD.md` (how a change is judged),
`docs/MAP.md` (every crate and module, generated), `docs/SETUP.md` (running
the new and the original game), `docs/ROADMAP.md` (delivery stages, what is
done and what is next), then the `README.md` of the crate you touch. The maintainer's agents also follow `docs/WORKFLOW.md`
(direct commits, roadmap ownership, `magic/` artifacts, handoff). The root
`README.md` is for players, `CONTRIBUTING.md` for human contributors.

## Hard rules

1. **Nothing from the install, and nothing from `magic/`, enters a tracked file.**
   Game assets, extracted archives, screenshots of copyrighted art, save games,
   working notes and research journals live under `magic/` (gitignored) or
   `cyrus-config/` (gitignored). `git add -f` is never the answer. If unsure
   whether something counts, it does: leave it in `magic/` and say so.
2. **The install is read-only.** cyrus never writes into the game directory.
   Local state is `cyrus-config/` next to the working directory.
3. **Faithful by default.** Every behaviour that may differ from the original
   is registered under a fidelity `Area` and runs only in the configured `Mode`
   (`crates/cyrus-game/src/fidelity.rs`). The default of every area is
   `Faithful`. A deviation lives in a `Modern` branch, never in the shared path.
   Faithful means the same player experience: visuals, audio, controls,
   camera, game logic, timing of scripts and animations, and outcomes. It
   never means reproducing technical limits. Loading time, frame rate,
   memory use and responsiveness should be as good as the hardware allows;
   do not add loading screens, frame caps or stalls to match the original,
   and do not accept a slow implementation because the original was slow.
   Where the original's behaviour depends on its frame rate or load timing,
   reproduce the behaviour the player sees, not the mechanism.
4. **Do not guess.** Derive constants, formulas, and behavior from game data,
   original-game observations, or preservation evidence, and state the source.
   When in doubt, inspect the original game's decompilation to resolve the
   uncertainty. Publish recovered engine facts in redguard-preservation before
   relying on them here. If evidence is still incomplete, leave the behavior
   explicitly unsupported. Neither `// unverified` nor Modern mode authorizes
   invented behavior. Keep raw decompilation and research under `magic/`;
   implement from documented facts, never copied decompiled code.
   **Use installed data as the source of truth.** Before implementing content,
   look for its original configuration or asset definition. Read menu entries,
   labels, layout, selection flags, and action IDs from `MENU.INI` through
   `rgpre`; do not duplicate them in Rust tables. The same rule applies to other
   data-defined content. Preservation owns parsing and recovered engine rules;
   cyrus supplies presentation and the supported action handlers. Unimplemented
   actions remain explicitly unavailable without replacing the original data.
5. **Facts, not traces.** Tracked files carry no memory addresses, no
   executable offsets, no decompiler placeholder names, no "the original does X
   at ...". Engine facts are stated as facts and, where they are format or
   engine knowledge, contributed to redguard-preservation's `docs/` so the
   public record lives there.
6. **No piracy.** Documentation tells readers to buy the game (GOG). No
   installer links, keys or game files anywhere in the repository.
7. **Feed findings back to redguard-preservation.** When you find a bug in
   it, a new format or engine fact, documentation that conflicts with what
   the game data or the original shows, or anything it is missing, open a pull
   request against [redguard-preservation](https://github.com/michidk/redguard-preservation)
   with the fix or the documented fact and its evidence (counts, samples,
   observations). Preservation docs contain only durable format and engine
   facts. Put verification methods in the pull request or commit, and keep
   open questions, investigation plans, and cyrus implementation status out
   of preservation docs. Do not work around a preservation bug silently in
   cyrus: link the pull request from the cyrus change, and keep any workaround
   marked until the fix is released.

These rules govern what ships, not what may be said. That cyrus is informed by
reverse engineering of the original, and which projects were read, is stated
openly and credited in `README.md`. What stays out is the material itself:
decompilation, dumps, addresses, journals. `dev/scripts/publish-check.sh` (run
by `just gates`, the pre-commit hook and CI) greps the tracked tree for those
shapes; passing it proves nothing about where code came from.

## Repository map

`docs/MAP.md` lists every crate and module with its purpose; it is generated
from the sources (`just genmap`) and the gates fail when it is stale. Beyond
the crates: `dev/docker/retro` runs the original game under DOSBox-X,
`dev/docker/dev` runs cyrus on a virtual display; `dev/scripts/` holds the
gates, `publish-check`, `genmap`, the run and compare scripts; `dev/fixtures/`
the replay fixtures; `.agents/skills/` the project skills; `magic/` is the
untracked working memory. Each crate has a `README.md` with its commands.

## Changes

- `just setup` once per clone (pre-commit hook, local agent config,
  cargo-hauler). With cargo-hauler, agents' cargo commands and the build
  scripts share one queue; `hauler status` shows what is running. Long
  builds may return a ticket: `hauler await cc-N`, never retry them.
- Full workspace builds take a long time. During development, run the narrowest
  relevant test and build small, isolated test scenes that exercise one system
  without loading the full game. Run `just gates` before the commit.
- Conventional Commits: `type(scope): description`. The scope is the crate
  without its prefix (`game`, `web`, `mcp`), the module (`scene`, `player`,
  `loading`), or `dev`, `docs`, `roadmap`, `agents`, `repo`.
- Gates before every commit: `just gates` (publish-check, generated map
  current, fmt, clippy with `-D warnings`, tests). Tests that need the install
  skip when it is absent and say so; with an install linked, a skip fails the
  gate, and the suite runs once more with the install unreachable.
- Say what was verified and how: a test, a headless run, a replay compared
  with the original. "It compiles" is not verification. Say what was not
  checked, and whether the root cause or a symptom was fixed.
- Label every claim about the game: **Verified** (game file, the original
  observed, preservation doc), **Inferred** (follows from such facts) or
  **Unknown**. An unlabelled claim is Unknown.
- Keep `docs/` and this file in sync with behaviour changes. Do not add new
  top-level markdown files without a reason stated in the commit.
- Bevy is pinned in `Cargo.toml` (`0.20`); do not migrate minor versions as a
  side effect. The `bevy-ecs` project skill carries the API conventions.

## Structure and quality

- Keep public APIs small and group crates by concern: assets, world, rendering,
  player, and control. `rgpre` owns formats and engine facts. Agree on file
  ownership before a crate split or other change across shared boundaries.
- Keep dependencies one-way. Core rules must not depend on rendering, a window,
  or networking. Use the browser build to check these boundaries.
- Document public APIs with their invariants, units, and provenance where
  applicable. Use explicit names that make the code readable without tracing
  unrelated modules.
- Delete dead and commented-out code. Give each TODO an owner or an issue.
  Keep rustfmt and clippy green; explain every lint allowance.
- Test contracts through public interfaces. Tests requiring an unavailable
  install must report that they skipped, rather than silently passing.
- Include the relevant file, world, and field in errors. Invalid or unsupported
  install data must produce a contextual error, not a panic.
- Prefer deletion and existing code over new abstractions. Before a broad
  change, establish any missing responsibility or dependency boundary that the
  change requires.

## References and playtesting

Default windowed and browser launches play the installed 3dfx splash and title,
then stop at the book menu. It supports Options, Display, Sound, Controls,
Movies browsing, keyboard navigation, and Quit. Native Movies playback uses
installed Smacker files and requires FFmpeg on PATH. New Game, saves, and browser
cinematic playback remain unavailable. Startup uses original-file assets, never
captured playback.
Preservation owns extraction and rendering facts; cyrus owns presentation.

For agent work, choose the entry point explicitly:

- `cargo run -p cyrus -- --skip-startup` skips splash/title to the book menu.
- `cargo run -p cyrus -- --scene START` skips startup and loads that world directly.
- `--headless --scene START` runs preload, export, and control without a window.
- MCP `skip_startup` skips presentation to the menu; MCP `load_scene {name}`
  bypasses presentation or leaves the menu to enter a world.
- Browser `?scene=START` loads a world directly. Its default URL runs startup.

Control `state` reports `Loading`, `Menu`, or `InGame`. Check it before driving
player input. Menu navigation uses Up/Down and Enter or the installed primary menu key.
Skipping startup does not enable gameplay. `state.menu` exposes the installed
entries, action IDs, readiness, selection, and unavailable-action message.
Wait for `state.menu.ready` before sending menu input. Settings use original action
units and persist outside the install in native `cyrus-config/menu.json` or
browser localStorage `cyrus-menu`. Existing player input consumes edited bindings,
including direct world launches. Subtitle and interlace preferences affect native
cinematics. Native effects, speech, and CD music consume the saved Sound preferences;
the menu plays its installed CD track.
`state.menu.movie` reports cinematic loading and one-based frame progress;
menu input is suspended during playback. A new key press uses the installed
forward skip points, then returns to the same selected entry.

Book turns and camera moves suspend navigation. Readiness becomes true after the
final settling update. Quit plays the authored close before exiting. Menu motion
uses original MENU.ROB frames and preservation camera/depth rules; labels and
widgets stay data-driven. Verify edits, restart persistence, and malformed-state
protection through native MCP. Keep installed labels, visibility flags, widget
positions, key names, and artwork data-driven; publish newly recovered menu rules
in preservation before using them here.
Keep each worktree's Cargo target directory separate so verification cannot use
another checkout's binary.

Reference clones (`just reference`, under `magic/reference/`) are research
hints, never proof; `docs/WORKFLOW.md` lists them and how to use them. The
`cyrus-playtest` skill is the loop for running cyrus, driving it over MCP and
comparing with the original (`just retro`, `just replay <fixture> <label>`;
`dev/fixtures/` holds the fixtures, so a comparison is something the next
agent can rerun).
