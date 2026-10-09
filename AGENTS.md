# cyrus — agent contract

cyrus is a faithful rebuild of *The Elder Scrolls Adventures: Redguard* (1998)
in Rust and Bevy. It reads the player's own GOG install at runtime and ships no
game data. The original game, run from that install, is the specification.
Redguard's file formats and engine facts are owned by the sibling project
[redguard-preservation](https://github.com/michidk/redguard-preservation)
(crate `rgpre`); cyrus consumes that crate and sends format discoveries back there.

Read in this order: this file, `docs/METHOD.md` (how a change is judged),
`docs/SETUP.md` (running the new and the original game), `docs/ROADMAP.md`
(what is next). `README.md` is for players.

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
   public record lives there. `scripts/publish-check.sh` greps for the shapes.
6. **No piracy.** Documentation tells readers to buy the game (GOG). No
   installer links, keys or game files anywhere in the repository.

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
- `docker/retro` — the original game under DOSBox-X with Voodoo emulation,
  streamed over noVNC. `docker/dev` — a virtual display with software Vulkan
  for running cyrus where there is no GPU or screen.
- `scripts/` — `gates.sh` (what must be green), `publish-check.sh`,
  `run-retro.sh`, `run-game.sh`, `build-web.sh`, `run-web.sh`.
- `.agents/skills/` — project skills (`.claude/skills` links here).
- `magic/` — untracked working memory: the GOG installer, the extracted game,
  research, journals, specs. Its own `README.md` and `METHODOLOGY.md` live there.

## Workflow

- Once per clone run `just hooks`; the pre-commit hook refuses working memory and game files.

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
