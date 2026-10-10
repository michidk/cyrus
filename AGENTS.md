# cyrus — agent contract

cyrus is a faithful rebuild of *The Elder Scrolls Adventures: Redguard* (1998)
in Rust and Bevy. It reads the player's own GOG install at runtime and ships no
game data. The original game, run from that install, is the specification.
Redguard's file formats and engine facts are owned by the sibling project
[redguard-preservation](https://github.com/michidk/redguard-preservation)
(crate `rgpre`); cyrus consumes that crate and sends format discoveries back there.

Read in this order: this file, `docs/METHOD.md` (how a change is judged),
`docs/SETUP.md` (running the new and the original game), `docs/ROADMAP.md`
(delivery stages, what is done and what is next), then the `README.md` of the
crate you touch. The maintainer's agents also follow `docs/WORKFLOW.md`
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
   observations). Do not work around a preservation bug silently in cyrus:
   link the pull request from the cyrus change, and keep any workaround
   marked until the fix is released.

These rules govern what ships, not what may be said. That cyrus is informed by
reverse engineering of the original, and which projects were read, is stated
openly and credited in `README.md`. What stays out is the material itself:
decompilation, dumps, addresses, journals. `dev/scripts/publish-check.sh` (run
by `just gates`, the pre-commit hook and CI) greps the tracked tree for those
shapes; passing it proves nothing about where code came from.

## Repository map

- `crates/cyrus` — the binary: argument parsing, plugin assembly, `--headless`.
- `crates/cyrus-game` — the game as Bevy plugins; no window assumptions.
  `install.rs` (find the install), `loading.rs` (startup preload with progress
  bar, parses every known file through `rgpre`), `scene.rs` (WORLD.INI world resolution, full-world export and
  load), `player.rs` (controllers behind the fidelity switch), `fidelity.rs`,
  `startup/` (installed splash/title playback, Options, and Movies browsing),
  `settings.rs` (`cyrus-config/settings.toml`), `sound.rs` (SFX effects, RTX speech),
  `music.rs` (CD tracks per world), `control.rs` (TCP control server).
- `crates/cyrus-control` — the control protocol types and a blocking client.
- `crates/cyrus-web` — browser build (wasm32). Shows scenes the native game
  exported to `cyrus-config/cache/`, served next to the page. Not a default
  workspace member; `just web-build` / `just run-web`. Portable parts of
  `cyrus-game` (`startup`, `animation`, `environment`, `fidelity`, `player`, `settings`) are what it reuses; keep
  install access, threads and sockets behind `cfg(not(target_arch = "wasm32"))`.
- `crates/cyrus-mcp` — MCP stdio server; `.mcp.json` (Claude) and
  `.codex/config.toml` (Codex) register it. Tools: ping, state, screenshot,
  teleport, input, set_fidelity, skip_startup, load_scene, play_sound,
  play_speech, stop_sounds, quit.
- `dev/docker/retro` — the original game under DOSBox-X with Voodoo emulation,
  streamed over noVNC. `dev/docker/dev` — a virtual display with software
  Vulkan for running cyrus where there is no GPU or screen.
- `dev/scripts/` — `gates.sh` (what must be green), `publish-check.sh`,
  `run-retro.sh`, `run-game.sh`, `build-web.sh`, `run-web.sh`, `compare.sh`,
  `smoke-mcp.sh`. `dev/hooks/` — the pre-commit hook `just setup` installs.
- `.agents/skills/` — project skills for Codex and Claude Code (`just setup`
  links `.claude/skills` here). `.mcp.json` registers the MCP server for
  Claude Code and other tools that read it; `.claude/` and `.codex/` are
  untracked local config.
- Each crate has a `README.md` with its role, modules and commands.
- `magic/` — untracked working memory: the GOG installer, the extracted game,
  research, journals, specs. Its own `README.md` and `METHODOLOGY.md` live there.

## Changes

- `just setup` once per clone (pre-commit hook, local agent config,
  cargo-hauler). With cargo-hauler, agents' cargo commands and the build
  scripts share one queue; `hauler status` shows what is running. Long
  builds may return a ticket: `hauler await cc-N`, never retry them.
- Conventional Commits: `type(scope): description`. Scopes: `game`, `scene`,
  `player`, `fidelity`, `control`, `mcp`, `docker`, `docs`, `skills`.
- Gates before every commit: `just gates` (publish-check, fmt, clippy with
  `-D warnings`, tests). Tests that need the install skip when it is absent
  and say so.
- Say what was verified and how: a test, a headless run, a screenshot
  compared with the original. "It compiles" is not verification. Say what was
  not checked, and whether the root cause or a symptom was fixed.
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
Movies browsing, keyboard navigation, and Quit. New Game, saves, and cinematic
playback remain unavailable. Startup uses original-file assets, never captured playback.
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
including direct world launches. Audio, subtitle, voice, and interlace preferences
persist; playback integration remains pending. Verify edits, restart persistence,
and malformed-state protection through native MCP. Keep installed labels,
visibility flags, widget positions, key names, and artwork data-driven; publish
newly recovered menu rules in preservation before using them here.
Keep each worktree's Cargo target directory separate so verification cannot use
another checkout's binary.

Reference clones (`just reference`, under `magic/reference/`) are research
hints, never proof; `docs/WORKFLOW.md` lists them and how to use them. The
`cyrus-playtest` skill is the loop for running cyrus, driving it over MCP and
comparing with the original (`just retro`, `just compare`).
