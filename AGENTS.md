# cyrus — agent contract

cyrus is a faithful rebuild of *The Elder Scrolls Adventures: Redguard* (1998)
in Rust and Bevy. It reads the player's own GOG install at runtime and ships no
game data. The original game, run from that install, is the specification.
Redguard's file formats and engine facts are owned by the sibling project
[redguard-preservation](https://github.com/michidk/redguard-preservation)
(crate `rgpre`); cyrus consumes that crate and sends format discoveries back there.

Read in this order: this file, `docs/METHOD.md` (how a change is judged),
`docs/MAP.md` (every crate and module, generated), `docs/SETUP.md` (running
the new and the original game), `docs/DIFFERENCES.md` (accepted differences
from the original), then the `README.md` of the crate you touch.
[Delivery issues](https://github.com/michidk/cyrus/issues/92) and
[SOUP #4](https://github.com/michidk/cyrus/issues/4) own remaining work,
acceptance, status and session claims. The maintainer's agents also follow
`docs/WORKFLOW.md` (direct commits, issue ownership, `magic/` artifacts, handoff). The root
`README.md` is for players, `CONTRIBUTING.md` for human contributors.

## Hard rules

1. **Nothing from the install, and nothing from `magic/`, enters a tracked file in cyrus.**
   Game assets, extracted archives, screenshots of copyrighted art, save games,
   working notes and research journals live under `magic/` (gitignored) or
   `game-config/` (gitignored). `git add -f` is never the answer. If unsure
   whether something counts, it does: leave it in `magic/` and say so.
2. **The install is read-only.** cyrus never writes into the game directory.
   Local state lives where `crates/cyrus-config` says: `CYRUS_CONFIG_DIR`, else an
   existing `game-config/` in the working directory (a checkout; `cargo` pins it),
   else the platform config, cache and state directories.
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
   with the fix or documented fact. Preservation docs contain only durable
   format and engine facts, stated directly without verification labels or
   research narrative. Put supporting samples, observation logs, traces,
   debugger details, and validation-only counts in the pull request or commit.
   Stable counts that are part of the format may remain in the docs as direct
   facts. Keep open questions, investigation plans, and cyrus implementation
   status out of preservation docs. Do not work around a preservation bug
   silently in cyrus: link the pull request from the cyrus change, and keep any
   workaround marked until the fix is released.

These rules govern what ships, not what may be said. That cyrus is informed by
reverse engineering of the original, and which projects were read, is stated
openly and credited in `README.md`. What stays out is the material itself:
decompilation, dumps, addresses, journals. `dev/scripts/publish-check.sh` (run
by `just gates`, the pre-commit hook and CI) greps the tracked tree for those
shapes; passing it proves nothing about where code came from.

## Private research archive

Research documentation and decompiler output must be committed and pushed to
the private [redguard-decomp](https://github.com/michidk/redguard-decomp)
repository before a research task is handed over. Archive new or updated
specifications, journals, task notes, handoffs, text observations, decompiled
C-like exports, and supporting analysis records from `magic/`. A local-only
copy does not complete the handoff.

Use a separate checkout, read its `AGENTS.md`, preserve source paths and hashes,
and add a dated snapshot for changed evidence. Update its research index,
commit to `main`, push, and verify the remote commit. Record the archive commit
in the local task handoff. Keep the working files under gitignored `magic/`;
do not stage them in cyrus.

Game executables, installers, assets, extracted game scripts, screenshots,
audio, video, manuals, saves, binary dumps, Ghidra databases, credentials,
dependencies, and build caches stay local. The private archive accepts research
text, decompilation, and analysis records. Durable engine and format facts
still belong in redguard-preservation; implementation belongs here.

## Repository map

`docs/MAP.md` lists every crate and module with its purpose; it is generated
from the sources (`just genmap`) and the gates fail when it is stale. Beyond
the crates: `dev/docker/retro` runs the original game under DOSBox-X,
`dev/docker/dev` runs cyrus on a virtual display; `dev/scripts/` holds the
gates, `publish-check`, `genmap`, the run and compare scripts; `dev/fixtures/`
the replay fixtures; `.agents/skills/` the project skills; `magic/` is the
untracked working memory. Each crate has a `README.md` with its commands.

## Changes

- Agents own implementation, playtesting, acceptance evidence and issue closure
  autonomously. Do not require human testing, approval or checkbox changes to
  complete work. Close an issue when its full scope and acceptance criteria are
  verified, linking the landing commits and evidence; update completed substeps
  while broader work remains open. Any maintainer personal-verification index
  is optional and separate from agent acceptance and issue closure. Resolve
  dependencies with other agents and available tools; document genuine external
  blockers and continue independent work without waiting for human involvement.
- `just setup` once per clone (pre-commit hook, local agent config,
  cargo-hauler). With cargo-hauler, agents' cargo commands and the build
  scripts share one queue with at most three concurrent builds; `hauler status`
  shows what is running. Long builds may return a ticket: `hauler await cc-N`,
  never retry them.
- Full workspace builds take a long time. During development, run the narrowest
  relevant test and build small, isolated test scenes that exercise one system
  without loading the full game.
- Conventional Commits: `type(scope): description`. The scope is the crate
  without its prefix (`game`, `web`, `mcp`), the module (`scene`, `player`,
  `loading`), or `dev`, `docs`, `agents`, `repo`.
- Match verification to the change before committing:
  - Documentation-only changes, including Rust doc comments, need
    `dev/scripts/publish-check.sh` and `git diff --check`. Also run
    `dev/scripts/genmap.sh --check` when module documentation may affect
    `docs/MAP.md`. Do not run Cargo for these changes.
  - Rust, Cargo, build-script and gate changes need the narrowest relevant test
    during development, then `just check` and `just verify <crate>` for each
    affected package after the final relevant edit. Run relevant script tests
    directly for build-script and gate changes.
    Tests that need the install skip when it is absent and say so; with an
    install linked, a skip fails the gate. CI verifies the no-install path.
  - Runtime and fidelity changes also need the relevant headless run, replay or
    original-game comparison. Generic workspace tests do not replace it.
- Use the fastest sufficient gate while iterating. `just check` runs publish,
  generated-map and formatting checks without compiling. `just verify <crate>`
  runs that crate's tests and clippy (for example `just verify game`). Use
  `just gates-lints` or `just gates-test` when debugging one stage. Run the
  full `just gates` only for changes that need workspace-wide local verification,
  such as workspace dependency or shared build configuration changes. CI runs
  the full workspace checks after every push; ordinary package edits do not
  require duplicating that full run locally.
- Do not repeat `just gates` when the tested tree and gate inputs are unchanged.
  A commit, wait, or rebase alone is not a reason to rerun it. CI supplies the
  full-workspace check after every push.
- Say what was verified and how: a test, a headless run, a replay compared
  with the original. "It compiles" is not verification. Say what was not
  checked, and whether the root cause or a symptom was fixed.
- Label every claim about the game: **Verified** (game file, the original
  observed, preservation doc), **Inferred** (follows from such facts) or
  **Unknown**. An unlabelled claim is Unknown.
- Keep `docs/` and this file in sync with behaviour changes. Do not add new
  top-level markdown files without a reason stated in the commit.
- Never commit or push plans (implementation plans, generated planning
  output, `docs/plans/`). Keep them in untracked working memory or in the
  conversation; only the resulting changes and `docs/` facts belong in git.
- Bevy is pinned in `Cargo.toml` (`0.20`); do not migrate minor versions as a
  side effect. The `bevy-ecs` project skill carries the API conventions.

## Structure and quality

- Group code by responsibility and default to modules within existing crates.
  A subsystem or Bevy plugin does not need its own crate. `rgpre` owns formats
  and engine facts. Agree on file ownership before a crate split or other
  change across shared boundaries.
- Extract a crate when it provides a concrete benefit: independent consumers,
  a separate build artifact, dependency or platform isolation, an enforceable
  architectural boundary, or a measured build improvement. Before extracting,
  state what becomes possible or prohibited and why a module is insufficient.
  File size, a new feature name, or speculative reuse alone does not justify a split.
- Keep each crate cohesive with a small public interface and one-way dependencies.
  If extraction exposes many internals, passes an owner's entire state across
  the boundary, or requires a generic shared-types crate to avoid a cycle,
  reconsider ownership before splitting. Prefer modules for tightly coupled
  code that shares dependencies and usually changes together.
- Verify the claimed benefit of a split. Use an isolated consumer for dependency
  isolation, relevant target builds for platform boundaries, and measurements
  for build-speed claims. Workspace feature unification can hide dependency
  leaks. Preserve useful existing boundaries; do not merge or split crates
  merely to reach a preferred crate count.
- Keep dependencies one-way. Core rules must not depend on rendering, a window,
  or networking. Run `dev/scripts/check-core-boundaries.sh` for an isolated
  consumer check and the browser build for platform boundaries. The consumer
  check also keeps the interpreter free of Bevy/assets, app helpers free of
  gameplay, and rendering free of the audio and startup crates.
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

Use the installed game, original-game observations, and redguard-preservation
for evidence. Put raw decompilation, traces, screenshots, and working notes in
`magic/`. Put durable format and engine facts in redguard-preservation. Keep
implementation status in issues, task artifacts, and crate documentation.

For browser checks, pass the install explicitly with
`just run-web --install /absolute/path/to/Redguard` or
`just run-viewer-web --install /absolute/path/to/Redguard`. Do not rely on a
remembered browser selection.

Use the smallest entry point needed for the check:

- `cargo run -p cyrus -- --skip-startup` opens the menu.
- `cargo run -p cyrus -- --scene START` loads a world directly.
- `cargo run -p cyrus -- --headless --scene START` runs without a window.
- MCP `skip_startup` and `load_scene` provide the same shortcuts over control.

Check the control `state` before sending input. Use `just replay <fixture>
<label>` for repeatable comparisons. Compare the same scene, position, inputs,
and timing in cyrus and the original. Record the evidence in the issue or task
artifact. Do not add recovered game behavior to this file.
