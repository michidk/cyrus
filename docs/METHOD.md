# METHOD — how cyrus is built

The original game, run from the player's own install, is the specification.
cyrus is a faithful, modern implementation of it: the same scenes, scripts,
rules, timing and look, written as idiomatic Rust and Bevy. Modern deviations
exist, but only behind the fidelity switch and only as additions on top of a
faithful path that stays intact.

## The rule for every change

A change is right when it makes cyrus more like the original, or fixes a bug,
with evidence, in one small piece, with the gates green. Evidence means one of:

- a game file, read through `rgpre` (and its published format docs);
- an observation of the original game, reproducible from a scene, a position
  and an input sequence, ideally with a DOSBox-X screenshot next to a cyrus one;
- a documented engine fact in redguard-preservation's `docs/engine/`.

A guess is allowed only when marked `// unverified` in code and named in the
commit message. It is never the default of a fidelity area.

## The loop

1. **Reference first.** Reproduce the behaviour in the original (`just retro`),
   write down scene, position, inputs and what happens. Find the data that
   drives it (INI, RGM scripts, model flags) before writing code.
2. **Spec in words.** Formulas, units, constants and branch conditions as a
   short note in the commit or the docs. The spec carries no code.
3. **Implement idiomatically.** Bevy ECS, plain Rust. Structure is free;
   behaviour is not.
4. **Compare.** Same scene, same inputs, screenshot or state dump from both.
   Put the comparison, or what could not be compared, in the commit message.
5. **Gates green, commit small.** `just gates`; one logical change per commit.

World-loading checks enumerate WORLD.INI and export every entry, including
worlds sharing a map with different palettes. Successful export proves loading
coverage; visual fidelity still requires comparison with the original.

## The fidelity switch

`Fidelity` is a resource with one `Mode` per `Area` (`player_controller`,
`camera`, `graphics`; extend the enum when a new deviation needs one). Systems
are gated with `in_mode(area, mode)`. Rules:

- The faithful path is complete on its own; the modern path may reuse it.
- A modern path never changes shared data layouts or scene content.
- Defaults are `Faithful`; `cyrus-config/settings.toml` and the control
  command `set_fidelity` flip them at runtime.

## Where things live

- Format and engine knowledge: redguard-preservation (`docs/formats`,
  `docs/engine`, the parsers). cyrus does not re-document formats.
- cyrus behaviour and architecture: `docs/` here, kept short, present tense.
- History: git. No changelogs, no status narratives in the tree.
- Working notes, research, journals: `magic/` (untracked).

## Gates

`just gates` runs: `cargo fmt --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, and `cargo test --workspace`. Install-dependent tests skip without an install
and print that they did. CI mirrors the same list.

## Not in scope

Original executable code, assets or audio in any form; distributing anything
from the install; multiplayer; content the original game did not have.
