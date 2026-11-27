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
for timing comparisons and debugger traces to establish call order and values. Combat pacing
also consumes an absolute cached BIOS sample. Equal effective measurement intervals
and frame scales do not establish equal pacing phase.
`original_combat_pacing.read_combat_pacing` retains that sample, the receiver
allocation index, distance, action, RNG state and independent divisors from local
stopped bindings. A tracee read establishes use; an endpoint alone does not.
Compare the wrapping pacing sum and signed remainders before attributing an
animation difference to selection logic.
Native stopped replay can require `cached_bios_samples`, containing the observed
entry sample followed by one absolute sample per completed body. The adapter
requires `state.clock.bios_sample` at that exact phase; absent or differing
samples reject before projecting outcomes or sending the next input. Effective
BIOS deltas never substitute for these observations. The native observer and
absolute-origin control must be available before this gate can pass.
`combat_pacing` optionally supplies one list per entry/body boundary alongside
that absolute sample tape. Each expectation names a unique inspected actor,
its signed `allocation_index`, independently observed `cached_bios_sample`,
and whether `selection` was reached. The driver
requires that actor's `combat_pacing.ordinary_update` to identify the current
body and its `cached_bios_sample` to equal that decision's measured input.
A later body-boundary clock sample is checked separately: intervening refreshes
can change it without a new FPS measurement (preservation player timing,
commit `2d80d944d430c61cd5d8e3232104333d65185247`). It checks the
signed wrapping `pacing_sum`, selection sum and checked counter/pacing remainders, preserving null
for invalid division. Null selection establishes combat entry only; it cannot
satisfy a selection expectation. Rejected observations remain in execution
evidence and block subsequent input. `NativePhaseError.evidence` retains the stopped
clock, expected sample, absolute ordinary update and actual actor pacing records
even if the session constructor rejects. Paired reports serialize this as
`phase_rejection`, so a rejected origin does not erase its observation.
This observation contract does not supply an absolute-origin command. `validate_bios_origin_tape` checks that entry plus
effective intervals reproduces every observed absolute sample, including unsigned
wrap. An origin-only setup cannot replay a tape with later sample discontinuities;
retain the original intervals and require a supported separate control or a new
recording. Adapters must use a separately supported native
control before claiming aligned replay; they must never infer or patch it.

Verified paired execution leases the executable inode across entry, separate
first-body and combat runs. The lease survives removal of the installed path,
uses the verified bytes for each Linux launch, and releases its handle at the
end. Each game run still owns its own lock and process lifetime.
Original watchpoint backends can buffer `execution_sink` resume/observed-stop
events with monotonic host timestamps. `debugger_execution_timing` validates
alternating pairs; these durations include stop-detection latency and never
supply game ticks or natural cadence. Keep sample/baseline writes, the actual
remainder addition, input acknowledgements and terminal releases together.
`replay_clock_inputs(..., entry_bios_sample=...)` requires continuous independent
measurement witnesses before exporting an origin-alignable absolute tape.
Without that argument its clock inputs remain diagnostic and do not establish
absolute phase. `validate_recording_inputs` rejects a horizon that truncates
an acknowledgement or leaves a held input without a terminal release.

`CombatTimingEvents` orders independently stopped combat-clock observations by
sequence and contiguous body number. A decision-cache read or write requires
its own tracee-access witness and sample; an entry assignment must agree with
the observed raw-clock source. Actor pacing reads additionally identify the
receiver and actual field read. Baseline refreshes, input acknowledgements and
body boundaries remain separate events. A later boundary sample cannot supply
a missing decision read. Executable/receiver bindings remain local evidence,
and a global cache assignment alone does not establish which actor consumed it.
`DecisionCacheHistory` binds subsequent reads to the independently resolved
entry receiver and per-body invocation occurrence; missing entries, stale bodies,
wrong receivers and changed decision samples reject. `combat_read_sample`
projects only the identified actor-field read. `measurement_from_baseline`
checks the calculation's baseline-store witness against the retained pre-body
baseline and remainder; unchanged-count refreshes and unequal timer reads reject.

`NativeFrameSession(clock_timelines=...)` accepts one measured timeline per
body using the #366 supported `ReplayFrame.clock_timeline` contract. Fixtures
contain `stored_sample`, `world`, ordered `decisions` with `index`, `name` and
`occurrence`, and `before_decisions`/`after_decisions` sample lists. `decision.index` is the original retained `runtime_identifier`, not a native
registry slot. Native admission uniquely resolves `world + runtime_identifier +
name` to a current-generation actor ID and rejects absent or ambiguous matches.
The driver forwards that original identity unchanged; it must not substitute a
native registry index.
The driver adds the actual script generation, absolute expected update and
observed pre-body BIOS sample as the command's compare-and-set fields. Effective
`elapsed_bios_ticks` stays mandatory and separate. It retains the submitted
command/reply and actual stopped clock, requires one input-boundary acknowledgement
with the current generation, next absolute update, zero relative offset and no
unexpected key action, checks generation stability, and validates the actual stopped
endpoint against the independently observed boundary tape. Fixture execution
alone does not establish per-decision or original/native outcome acceptance.

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

Paired outcome checkpoints can assert exact named `actor_health` values and
`separation_l1_q8: {opponent, value}`. Separation sums absolute differences on all
three Q8 position axes, including height, and requires both observed positions.
Use observed values at each checkpoint with player health and pose assertions
to distinguish a damage-free retreat from opponent movement or a hit. Missing
measurements reject the assertion. These equalities introduce no range threshold.

Input-only paired segments may specify `updates` beside an `input` batch to
consume exactly that many ordinary bodies. Input offsets may equal this duration:
the driver acknowledges those terminal inputs without consuming another body.
Without `updates`, the legacy duration is the largest input offset plus one.
Offsets beyond an explicit duration are rejected. Use this form to retain final
key releases at a recording's last observed boundary.
Outcome adapters call `require_released_inputs` after consuming the recording,
including its terminal boundary. It requires ordered, accepted same-frame input
receipts and an acknowledged release for every held key; a matching health
endpoint cannot replace missing release evidence.

Native paired frame sessions retain each observed consumed Q8 scale and full
public clock continuation. They reject a replay acknowledgement whose consumed
scale differs and reject clock changes during observation-only reads. Supply
recorded per-update BIOS intervals for the measurement/smoothing continuation;
omitting them uses simulated BIOS timing and establishes only a diagnostic scale
replay. Equal movement scales alone do not prove matching clock continuations.

`original-frame-session.py`'s `OwnerReferenceObserver` correlates an observed
retained owner token with a semantic census across world replacement. It records
an absent owner separately from a reassigned source or changed identity at the
same token. Census absence does not prove deallocation or continued lifetime;
observe allocation/removal separately. A matching actor slot in another world
does not establish owner identity, and absent owners are never dereferenced by
this correlation helper.

Paired projections expose `native_source` separately from the registry actor
census. `original-frame-session.py` provides `project_native_source` for decoded
`attributes`, `position_q8`, `orientation` and `script_context`. Use installed
attribute names and normalized script cursors/local offsets, with script and
string context identified semantically. This projection preserves the supplied
observations; its field names do not establish complete observation coverage.
Unobserved values stay null and reject comparison, even when both sides omit the
same measurement. The shared separate temporary uses `kind: separate_temporary`
and `lifetime: process_global`, without a registry owner identity. An ordinary
`registry_actor` source requires its independently bound semantic `owner`.
A matching numeric actor slot cannot substitute for either binding. Shell source
lifetime remains unsupported by this helper. Missing source projection on one
side rejects admission and replay reopening; both adapters must supply it before
claiming full source-state admission. Registry census agreement alone is partial
coverage. No source state is inferred from a departed semantic owner.

`original_source_state.py` reads all 256 source attributes, signed angle words,
row-major IEEE matrix bits, stored model position and complete authored local
and string counts through the stopped reader's bindings. Installed attribute
names identify known slots; unnamed slots retain their indices as data. Read
locals and relocated string pointers through the currently observed process
pools and instance byte offsets. Reusing an earlier decoded context cannot
establish the loaded context. Truncated reads, missing metadata, null string
pointers and strings beyond the observation bound reject the capture. Stored
orientation is observed directly, without rebuilding a matrix from angles.

`original_native_call.py` resolves a stopped encoded native call through the
observed runtime handler table and installed function names. It checks the
handler against the call record and retains receiver/source equality separately.
For hook dispatch without a bytecode record, its handler-table reader requires
the observed table count to equal the installed definition and rejects unknown
or shared handlers. A selected handler identifies the impending invocation, not
a completed call.
A retained source can belong to the preceding dispatch. Observe the handler
write and subsequent source assignment around the event before attributing an
RNG write to the current native receiver; a matching snapshot alone does not
establish that a call is still active. Executable bindings remain local.

`original_support_state.py` decodes a completed support result, keeping source
validity separate from retained normal, texture and metadata after a miss.
Callers must establish query ownership and completion before treating it as the
player's support result; an actor surface byte alone is insufficient. A partial
record, unsupported source kind or contradictory miss/height/classification
rejects. Executable field bindings remain local.

The original watchpoint backend can pair an access comparator with a write-only
comparator on the same bytes to distinguish reads from stores, including stores
that leave the value unchanged. Read-modify-write instructions classify as
writes. Callbacks run with the emulator stopped; observation budgets count
comparator hits, so a store consumes two hits. Disarm both comparators when the
observed lifetime ends. Reads performed by the external observer are separate
from guest execution and do not establish guest access.

`original_acquisition.py` binds named stopped RNG boundaries to its continuous
write history. `RandomWriteHistory.boundary` checks the measured state against
the last observed write and retains its observed draw count and timer context.
A missing final write rejects the boundary. `require_boundary` compares an
independently measured endpoint at that same lifecycle phase; matching a later
observed phase remains a rejection and does not infer the other engine's call
count. Counter equality alone cannot identify a restoration phase, since saved
counters can replace the pre-load values.

`compare-segment.py --restoration-history <local-witness.json>` requires an
ordered `initial_state`/`events` witness. Each `write` event supplies its observed
`draw`, `previous`, `state`, `phase` and `context`; each `boundary` supplies its
`name`, `draw`, `state` and `context`. Required boundaries are
`restored-publication` and `first-completed-body`, in that order. Construct this
witness from the same source/save-bound acquisition used by the adapters.
The driver rejects missing writes and checks held frame zero before admission.
After semantic entry admission, it opens separate no-input runs, rechecks the
entry and advances exactly one acknowledged body before checking the later RNG
boundary. Adapters must supply the observed first-body timing inputs. Only then
can the untouched combat tapes run from their admitted entries. Both phase
observations and available execution receipts survive rejection in the report;
RNG-phase success alone does not establish complete semantic or clock parity.

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

The original combat-driver bundle includes `original_sky_state.read_sky_state`
for stopped sky continuation observations. Supply locally established bindings
for activation, direction, signed speed, scroll offsets, BIOS baseline and the
opaque texture lifetime token. Observe direction writes across a load to
distinguish retained state from an intervening reroll; equal endpoint angles
alone do not establish retention. Asset identity comes from installed settings,
not from comparing process handles across runs.

`original_loading_state.read_loading_state` observes the loading-artwork policy
and retained frame index through the same stopped driver bundle. The index can
remain unchanged when artwork is disabled; it is not evidence of a new image
selection. Attribute selection only with the observed route and policy/index/RNG
write order.
