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

Do not guess. When the evidence above leaves behavior uncertain, inspect the
original game's decompilation and publish recovered facts in preservation's
docs. Keep raw decompilation and research under `magic/`; implement from the
documented facts. If uncertainty remains, leave the behavior explicitly
unsupported. An `// unverified` comment or Modern mode does not authorize a guess.

## The loop

1. **Reference first.** Reproduce the behaviour in the original (`just retro`),
   write down scene, position, inputs and what happens. Find the data that
   drives it (INI, RGM scripts, model flags) before writing code.
2. **Spec in words.** Formulas, units, constants and branch conditions as a
   short note in the commit or the docs. The spec carries no code.
3. **Implement idiomatically.** Bevy ECS, plain Rust. Structure is free;
   behaviour is not.
4. **Compare.** Same scene, same inputs, screenshot or state dump from both,
   from a fixture another agent can rerun (`just replay`). Then prove the
   comparison can fail: change one constant in cyrus and confirm it is caught.
   Put the comparison, the wrong-version check, or what could not be compared,
   in the commit message.
   Keep both captures until the pair is accepted or rejected and that decision
   is recorded on the owning issue. Delete captures only after that record exists.
5. **Verify, then commit small.** Run the checks required by `AGENTS.md`; one
   logical change per commit. Run `just check` and focused `just verify <crate>` checks
   after the final relevant edit; use full gates for workspace-wide changes. Do not run Cargo for documentation-only
   changes.

Every validated gameplay scenario needs an original-first, side-by-side run. Use
the same world, starting state and player inputs, timed in game frames rather
than host seconds. Record both sides' player and actor positions, flags, world,
health, prompts, animation groups and timing at matched checkpoints, plus paired
screenshots for visual behavior. Post both measurements and their differences on
the issue. A native-only replay pass is regression coverage, not fidelity
acceptance. Investigate a divergence and cover its fix with a regression test.

## Controller parity evidence

An isolated arithmetic test establishes only that calculation. To claim movement
parity, compare the complete player update from a reproducible starting state:
scene, settled position, heading, actor attributes, held inputs, animation state,
and timing inputs. Record fractional positions when integer displays hide a
difference. Check startup, steady movement, and release separately.

Measure simulated updates rather than treating host key-hold duration or video
frames as an update count. Debugger pauses change timing; use native recordings
for timing comparisons and debugger traces to establish call order and values.
If a helper-only prediction disagrees with the recording, trace the remaining
update before implementing or tuning constants to fit the endpoint.

`python3 dev/scripts/compare-player-position.py --reference reference.json
--native native.json` compares normalized position observations at equal elapsed
clock units. Each input is an array of `{elapsed_scale, position_q8}` records;
callers establish the same units, origin and coordinate space. The tool linearly
interpolates reference positions and reports the bracketing observations and
signed residuals. It refuses extrapolation and never interpolates animation or
controller state. Matching positions alone do not establish matching support
state or completed-update boundaries.

Input-only paired segments may specify `updates` beside an `input` batch to
consume exactly that many ordinary bodies. Input offsets may equal this duration:
the driver acknowledges those terminal inputs without consuming another body.
Without `updates`, the legacy duration is the largest input offset plus one.
Offsets beyond an explicit duration are rejected. Use this form to retain final
key releases at a recording's last observed boundary.

Native paired frame sessions retain each observed consumed Q8 scale and full
public clock continuation. They reject a replay acknowledgement whose consumed
scale differs and reject clock changes during observation-only reads. Supply
recorded per-update BIOS intervals for the measurement/smoothing continuation;
omitting them uses simulated BIOS timing and establishes only a diagnostic scale
replay. Equal movement scales alone do not prove matching clock continuations.

For original per-update recordings, run
`python3 dev/scripts/original-update-recorder.py --backend <local-backend.py> --elapsed <clock-units>`.
The Linux x86-64 backend `dev/scripts/original-update-watchpoint.py` uses
`ORIGINAL_UPDATE_CONFIG` to bind an ignored original state reader and a verified
ordinary-update-start write watchpoint. Executable bindings stay under `magic/`.
At this stopped boundary the previous ordinary update has completed. The recorder
reads twice without resuming, requires contiguous update counters, and checks the
clock difference against the preceding update's consumed step. It emits a baseline
and one snapshot per completed update, including animation, fall, support owner,
link and previous local position. A duplicate, gap, torn read or clock mismatch
rejects the recording; discard partial output after any failure. A clock read on
its own is never a boundary. The requested duration ends on consumed game-clock
progress; the host timeout only detects a stalled boundary. Verify executable
identity and the boundary's call order locally before accepting a new binding.
Synthetic controls run with `python3 -B dev/scripts/test-original-update-recorder.py`.

For actor-lighting comparisons, bind actor identity, selected model/frame,
transform and camera to the presentation being sampled. A stopped ordinary-update
receipt or matching animation group does not replace that presentation binding.
Check the native displayed model/frame as well as its logical animation state;
paused presentation can retain an earlier pose. Reject the crop when either
binding is missing or differs, before assigning pixel residuals to lighting.

World-loading checks enumerate WORLD.INI and export every entry, including
worlds sharing a map with different palettes. Successful export proves loading
coverage; visual fidelity still requires comparison with the original.

For settled browser/native frames, use identical SYSTEM.INI and WORLD.INI
inputs and record their hashes. Start the settling interval after the first
ordinary actor update: game-clock progress and scene readiness can precede
actor model preparation. `stopAfterWorldProgress` in `capture-cdp.mjs` enforces
that boundary before stopping animation callbacks for readback.

## The fidelity switch

`Fidelity` is a resource with one `Mode` per `Area` (`fog`, `sky`, `lighting`, `textures`,
`music` and `anisotropic_filtering`; extend the enum when a new deviation needs one). Areas cover
only how the game looks and sounds; deviations in behaviour, such as controls, movement or camera
handling, belong in a native mod. Systems
are gated with `in_mode(area, mode)`. Rules:

- The faithful path is complete on its own; the modern path may reuse it.
- A modern path never changes shared data layouts or scene content.
- Defaults are `Faithful`; `settings.toml` in the config directory and the control
  command `set_fidelity` flip them at runtime.

## Where things live

- Format and engine knowledge: redguard-preservation (`docs/formats`,
  `docs/engine`, the parsers). cyrus does not re-document formats.
- cyrus behaviour and architecture: `docs/` here, kept short, present tense.
- History: git. No changelogs, no status narratives in the tree.
- Working notes, research, journals: `magic/` (untracked).

## Gates

For ordinary package changes, run `just check` and `just verify <crate>`
for each affected package, including callers affected by a public API change.
The focused verification runs all package tests, including integration tests,
and clippy for all package targets. Build-script changes run their relevant
script tests. CI supplies workspace coverage; a full local run is required only
when the change needs workspace-wide verification.

For that full run, `just gates` runs `cargo fmt --check`, `cargo
clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.
Install-dependent tests skip without an install and print that they did. CI
mirrors the same list. `AGENTS.md` defines the smaller checks for documentation
changes and the required playtests for runtime behavior.

## Not in scope

Original executable code, assets or audio in any form; distributing anything
from the install; multiplayer; content the original game did not have.
