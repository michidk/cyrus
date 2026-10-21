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
    during development, then one `just gates` run after the final relevant edit.
    Tests that need the install skip when it is absent and say so; with an
    install linked, a skip fails the gate. CI verifies the no-install path.
  - Runtime and fidelity changes also need the relevant headless run, replay or
    original-game comparison. Generic workspace tests do not replace it.
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

The user authorizes agents to create their own rendering verification previews
and restart them as often as needed, without asking for each run. Reuse each
session's existing names, register its owning thread, and leave other sessions'
previews alone. Routine agent cleanup does not revoke this authorization.
A later explicit user instruction to keep a named preview stopped takes
precedence; do not create a replacement to evade it.

Default windowed and browser launches play the installed Windows Glide 3dfx splash and title,
then stop at the book menu. `--splash dos` (`?splash=dos`) plays the DOS overlay's shield
splash instead; `--music modern` mutes the CD data noise at the start of track 2. Both are
listed in `docs/DIFFERENCES.md`. It supports Options, Display, Sound, Controls,
Movies browsing, keyboard navigation, and Quit. Native Movies playback uses
installed Smacker files through the in-process decoder. Browser Movies uses
bounded original Smacker ranges and incrementally decoded first-track PCM with shared playback rules.
New Game enters the installed starting world and marker; Load temporarily starts
the same fresh world. Saves, full gameplay/story initialization and progression
unlocks remain unavailable. Native Quit closes the book and exits; browser Quit
attempts tab closure and leaves the menu running when refused. Startup uses original-file assets, never
captured playback. **Verified:** the metallic logo and orange swoosh come from
`3DfxSpl2.dll`; the full DOS Glide overlay contains the older shield variant.
Preservation owns extraction and rendering facts; cyrus owns presentation.

`cyrus-viewer` is a separate native/browser inspection application. It accepts
an install path or discovers default folders and uses generic textured rendering, without a
Faithful appearance claim. It shares `cyrus-assets` and `cyrus-app` with the game;
its controls do not change game preferences. See its crate README for asset
coverage, browser runtime loading, and preview limitations.

For browser testing, always supply the resolved install directly. Start the server
with `just run-web --install /absolute/path/to/Redguard` or
`just run-viewer-web --install /absolute/path/to/Redguard`. When using an existing
server, pass `?install=<encoded-install-service-URL>` on the application URL.
Do not ask the user to enter the location or rely on their saved browser selection.
The shared server default prefills the picker on both game and viewer pages.
The picker always appears; automate Connect to server for an unattended launch.
Test local folder selection when the task specifically needs to verify that flow.

Native game and viewer share the install picker in `cyrus-app`. CLI/environment
paths precede the remembered `cyrus-config/install.json` location and default
folder discovery. Windowed resolution failures show a path field, Browse and
Continue; a successful selection saves the resolved root outside the install.
Invalid remembered locations reopen the picker. Cancel leaves saved state alone.
Selection and loading use the existing native window without a launcher process.
Headless, world-listing and export commands retain command-line errors. Browser
selection supports a local GOG folder or an install-service URL. Local files stay
in browser File handles and are never uploaded. The shared localStorage key
`cyrus-install` stores either the service URL or `local`; local folders must be
selected again after reload. `?install=local` forces the picker. Choose the outer
GOG folder containing the game directory, DOSBOX and the disc files.

For agent work, choose the entry point explicitly:

- `cargo run -p cyrus -- --skip-startup` skips the splash and prepares the menu behind the title.
- `cargo run -p cyrus -- --scene START` skips startup and loads that world directly.
- `--headless --scene START` runs preload, export, and control without a window.
- MCP `skip_startup` skips presentation to the menu; MCP `load_scene {name}`
  bypasses presentation or leaves the menu to enter a world.
- Browser `?scene=START` loads a world directly. Its default URL runs startup.

Control `state` reports `SelectingInstall`, `Loading`, `Menu`, or `InGame`. Check it before driving
player input. Menu navigation uses Up/Down and Enter or the installed primary menu key.
**Verified:** the original default book menu has no pointer hover/click selection; do not add
label hit boxes. Controls capture accepts a left or right mouse-button release (codes 128
and 129), and a binding to either reads as the original's release edge: true for the update in
which the button goes up, never while held. Joystick codes (132 and up) and mouse-bound
inventory, log, map and sword actions are unsupported. Book turns, cover crossings and successful direction/key
capture feedback dispatch installed effects through the shared native/browser
sound path, using preservation's [cue rules](https://github.com/michidk/redguard-preservation/pull/34).
Animation sounds use installed SURFACE.INI remaps on native/browser when the actor
has a known SoundSurface. Known zero bypasses remapping; missing state remains
unsupported. Ground and SOUP hosts own surface assignment. Sound cleanup retains
the authored effect ID, and old browser sound banks require re-export.
Skipping startup retains required preparation behind the title and does not enable gameplay.
Menu New/Load wait behind installed destination artwork and a red percentage bar
for world preparation and asset dependencies. Bottom-right red title labels report startup
menu/HUD preparation. Those labels describe cyrus work. **Verified:** world-entry
loading shows the original red bar and gold lower-left percentage, without a
right-hand status message or pending asset filename. Menu world entry fades the loading
screen in, fades it out after loading and fades the first world view in, 12/256 per
installed nominal tick. Failed world entry reports a
contextual error and supports Escape back to the menu. In a world, `state.hud` reports health,
candle mode, visibility and frame, compass frame, power-up slots, logbook frame,
walk mode, auto-defend, prompt, selected item and count. **Verified:** owned
item-zero use toggles the shared compass flag once per held installed
Activate/Attack binding when supplied actor state permits it. Unknown actor
eligibility rejects use. Ordinary compass use reads logical-player control,
surface, animation and attributes. Ordinary prompt activation also reads live
receiver/player state, installed RTX and the held Activate binding. Standalone
player initialization establishes the shared lineup result. `PlayerLineUp`,
`PlayerPickupLineUp` and `PlayerParallelLineUp` run as native tasks that walk the
selected player to the target; object-side lineups and conversation progression
remain unavailable. World shell scripts (`COMMON` and the map's own script, plus the debug
shell when SYSTEM.INI `final_version` is off) run every update before the ordinary actors until
`KillMe`. `OpenShell` starts a named shell script. The `UseItem` native starts a general item's use script as a shell, within the item's
instance limit, and applies its drop list and use flags, and lights or puts out a torch; weapon
items draw the sword. `DrawSword` runs the weapon draw admission. No inventory screen calls
`UseItem` yet. `PlayerCollide` and `PlayerCollideStatic`
read the contact lists of the selected player's body pass. The installed quick
sword and quick health keys run in the world loop: the sword key draws, sheathes or uses the
first owned of the three swords, and the health key drinks the first owned potion. `CollidePlayerWeapon` and
attribute 126's weapon-tip contact test read the retained combat weapon endpoints. `AddFlatEffect` builds an environmental effect
from the installed zero-spread, speed-one form. `GolemSteam` puffs steam at a
random weapon attachment vertex of the Golem. `shoot` creates an object of an
installed script (a projectile) at the receiver's rotated offset; created objects run their script
every update until `KillMe` and survive saves. `shootPlayer` aims the created object at the selected player.
`DragonBreath` and `projectileAI`'s movement and hit response remain unavailable. `LightTorch`,
`UnlightTorch`, the torch burn countdown and the held-torch lighting and dousing progression run on
logical actor state; the held torch model and its light remain unpresented. Script caches need re-export
(schema 27). Scripts and
the HUD share session inventory
through `AddItem`, `DropItem`, `HaveItem` and `SelectItem`. Recursive prerequisites,
ordered acquisition/removal lists and starting acquisitions use that owner.
New-game/death resets and save restore remain unsupported. Scripted choice menus
(`menuNew`, `menuAddItem`, `menuProc`, `menuSelection`) draw as HUD text at the installed
menu origin, one font line apart, with the selected, normal and used alphas from
SYSTEM.INI; the font colour parameter beyond its alpha is not modelled. Re-export older HUD
caches for inventory definitions, all selectable artwork and menu text alphas. `state.script_hud.compass_on` reports the retained
session flag. Installed `ShowBitmap`,
`UnShowBitmap` and `AddLog` calls feed shared native/browser HUD state.
`state.script_hud` reports native dispatch readiness, power-up slots and accepted
logbook entry count or load error. Log text stays outside the install in native
`cyrus-config/logbook.json` or browser localStorage `cyrus-logbook`. Malformed
state and failed writes do not notify or overwrite accepted entries. The installed
log key opens the shared native/browser reading screen at the newest
accepted text. Installed up/down and A, Home/End and Page Up/Down scroll it; log,
Escape or installed D closes it. Simulation and gameplay input remain suspended
until the exit keys are released. Existing audio continues. Native `state.logbook`
reports readiness, scrolling and errors. The reader uses installed paper, small
fonts and rotating scroll geometry. Styles 0 to 4 and 6 to 9 draw as in the
original, and style 5 draws nothing as the original does. Manual editing,
new-game/death resets and save-game integration remain unsupported. Scroll lighting
and pixel-exact filtering still need comparison.
F8, Tab and F7 toggle
candle mode, walk mode and auto-defend. `state.menu` exposes the installed
entries, action IDs, readiness, selection, and unavailable-action message.
Wait for `state.menu.ready` before sending menu input. Settings use original action
units and persist outside the install in native `cyrus-config/menu.json` or
browser localStorage `cyrus-menu`. Existing player input consumes edited bindings,
including direct world launches. Subtitle and interlace preferences affect
cinematics. Native/browser effects and CD music, and native speech, consume the saved Sound
preferences; the menu plays its installed CD track. Browser CD tracks use exported
WAV files and installed world assignments.
`state.menu.movie` reports cinematic loading and one-based frame progress;
menu input is suspended during playback. A new key press uses the installed
forward skip points, then returns to the same selected entry.

Native `state.cheats` reports CHT load readiness, raw values and supported effects.
TCP/MCP `cheat {name, value?}` toggles or assigns state; `oracle` lists it and
`savecheats` writes `cyrus-config/REDGUARD.CHT`. Local state precedes the installed
read-only CHT file; malformed state blocks edits and saves. **Verified:** runtime
`neversaydie` supplies the documented PlayerDistance override to automatic world
updates and explicit script steps unless the caller supplies `distance_override`. Other effects, including
unknown `oddjob` behavior, report unsupported. F12 opens the shared console while
in a world; F12/Escape closes it. Gameplay accepts typed cheat names then Enter.
Browser `savecheats` uses localStorage `cyrus-cheats`; F1 Cheat files imports and
exports CHT. Startup SYSTEM.INI defaults replace task, animation and nodemarker.
TCP/MCP `console {open?, line?}` and `state.console` expose the shared editor.
`console save` persists CPF preferences outside the install; `console write` exports
retained output. Native CPF uses `cyrus-config/REDGUARD.CPF`; browser CPF uses
localStorage `cyrus-console`. Malformed local CPF blocks preference edits and saves.
The original command catalog lists unavailable handlers explicitly. `show world`
and sky-plane/wave controls operate on the current environment and retain session edits.

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
