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
crate you touch. The root `README.md` is for players.

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
4. **Provenance over plausibility.** A constant, formula or branch is either
   derived from a game file, from observing the original game, or from the
   preservation docs, and the code says which; or it is marked
   `// unverified` and listed in the commit message. No silent guesses, no
   "looks right". Prefer an explicit unsupported path to a plausible wrong value.
5. **Facts, not traces.** Tracked files carry no memory addresses, no
   executable offsets, no decompiler placeholder names, no "the original does X
   at ...". Engine facts are stated as facts and, where they are format or
   engine knowledge, contributed to redguard-preservation's `docs/` so the
   public record lives there. `dev/scripts/publish-check.sh` greps for the shapes
   and CI runs it.
6. **No piracy.** Documentation tells readers to buy the game (GOG). No
   installer links, keys or game files anywhere in the repository.

## Several agents at once

More than one agent (Claude Code, Codex, or several sessions of either) is
often working in this checkout at the same time, on the same `main`, with
no worktree. Silence is the failure mode; coordination is part of the work.

- **Before starting:** `git pull`, read `git status` and `git log -5`. An
  uncommitted change you did not make belongs to someone else; do not
  revert, stage, format or "fix" it. Find the owner (T3: `t3_thread_list`
  and `t3_thread_send`; otherwise ask the user) and announce what you are
  about to touch.
- **Claim files, not topics.** Say which files and crates you will edit and
  for roughly how long. Do not edit a file another agent has claimed until
  they report done. Shared files (`AGENTS.md`, `justfile`, `Cargo.toml`,
  `docs/`) are edited in small, additive hunks and announced.
- **Cargo is shared.** One `target/` directory, one lock. Do not kill cargo
  processes you did not start; do not change compiler setup (toolchain,
  `~/.cargo/config.toml`, system-library prefixes) without agreeing on the
  exact values with the other agents first, because a differing environment
  rebuilds every dependency for everyone.
- **Commit only your files**, by name, never `git add -A` while others have
  work in the tree. Push after each commit so the others can pull.
- **Report at milestones**, not per step: what landed (commit hash), what was
  verified and how, what was left out, and which files are now free. Treat
  a message from another agent as a fact about the tree, not as approval
  from the user.

## Repository map

- `crates/cyrus` — the binary: argument parsing, plugin assembly, `--headless`.
- `crates/cyrus-game` — the game as Bevy plugins; no window assumptions.
  `install.rs` (find the install), `loading.rs` (startup preload with progress
  bar, parses every known file through `rgpre`), `scene.rs` (WORLD.INI world resolution, full-world export and
  load), `player.rs` (controllers behind the fidelity switch), `fidelity.rs`,
  `settings.rs` (`cyrus-config/settings.toml`), `control.rs` (TCP control server).
- `crates/cyrus-control` — the control protocol types and a blocking client.
- `crates/cyrus-web` — browser build (wasm32). Shows scenes the native game
  exported to `cyrus-config/cache/`, served next to the page. Not a default
  workspace member; `just web-build` / `just run-web`. Portable parts of
  `cyrus-game` (`fidelity`, `player`, `settings`) are what it reuses; keep
  install access, threads and sockets behind `cfg(not(target_arch = "wasm32"))`.
- `crates/cyrus-mcp` — MCP stdio server; `.mcp.json` (Claude) and
  `.codex/config.toml` (Codex) register it. Tools: ping, state, screenshot,
  teleport, input, set_fidelity, load_scene, quit.
- `dev/docker/retro` — the original game under DOSBox-X with Voodoo emulation,
  streamed over noVNC. `dev/docker/dev` — a virtual display with software
  Vulkan for running cyrus where there is no GPU or screen.
- `dev/scripts/` — `gates.sh` (what must be green), `publish-check.sh`,
  `run-retro.sh`, `run-game.sh`, `build-web.sh`, `run-web.sh`.
- `.agents/skills/` — project skills for Codex and Claude Code (`just setup`
  links `.claude/skills` here). `.mcp.json` registers the MCP server for
  Claude Code and other tools that read it; `.claude/` and `.codex/` are
  untracked local config.
- Each crate has a `README.md` with its role, modules and commands.
- `magic/` — untracked working memory: the GOG installer, the extracted game,
  research, journals, specs. Its own `README.md` and `METHODOLOGY.md` live there.

## Workflow

- Once per clone run `just setup`; it writes the untracked local agent config
  (`.claude/`, `.codex/`): the skills link, the MCP pre-approval, the Codex server.

- `git pull` before starting. Commit completed, verified work directly to
  `main` and push; no feature branches or pull requests unless asked.
- Conventional Commits: `type(scope): description`. Scopes: `game`, `scene`,
  `player`, `fidelity`, `control`, `mcp`, `docker`, `docs`, `skills`.
- Gates before every commit: `just gates` (fmt, clippy with `-D warnings`,
  tests, publish check). Tests that need the install skip when it is absent
  and say so.
- Say what was verified and how: a test, a headless run, a screenshot
  compared with the original. "It compiles" is not verification.
- Keep the three docs in `docs/` and this file in sync with behaviour changes.
  Do not add new top-level markdown files without a reason stated in the commit.
- Bevy is pinned in `Cargo.toml` (`0.20`); do not migrate minor versions as a
  side effect. The `bevy-ecs` project skill carries the API conventions.

## Playtesting loop for agents

1. Start the game: `just run` (window) or `just run-novnc` (browser via the
   VibePod preview `cyrus`). `just run-headless` for loader and control only.
2. Drive it through the MCP tools, or `cyrus-control` directly on
   `127.0.0.1:7117`. Screenshots land in `cyrus-config/screenshots/`.
3. Compare against the original: `just retro` starts the GOG build in
   DOSBox-X behind the VibePod preview `cyrus-retro`. Note the scene, the
   position and what differs; put the comparison in the commit message.
4. Stop what you started (`just stop`).
