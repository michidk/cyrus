# cyrus-control

The control protocol between a running game and anything that drives it:
one JSON object per line over TCP, default `127.0.0.1:7117`.

```text
{"id":1,"cmd":{"type":"state"}}
{"id":1,"ok":true,"result":{"state":"InGame","scene":"START","player":{...},"fidelity":{...},"frame":120}}
```

Commands: `ping`, `state`, `screenshot {path}`, `teleport {x,y,z}`, `flycam {enabled, position?, look_at?}`,
`input {key, action: press|release|tap}` (Bevy `KeyCode` names),
`set_fidelity {area, mode}`, `skip_startup`, `load_scene {name}`, `play_sound {effect}`,
`play_speech {label}`, `stop_sounds`, `queue_movie {movie}`, `script_load_world {world, marker, heading}`, `quit`.

`skip_startup` goes to the book menu. `load_scene` skips presentation or
leaves the menu and enters a world. `state` includes `Loading`, `Menu`, or `InGame`.
`state.sounds` counts the effect and voice clips that are playing and, of those, paused
(`{playing, paused}`), so a save/load can be checked for sample continuation without a sound device.
`state.flycam` includes the scene-space camera position and quaternion rotation
in `[x,y,z,w]` order for repeatable view comparisons.

`Client` is a blocking helper for tests and tools. `server::listen` owns native
TCP framing and sends pending requests to the host. `cyrus/src/control.rs` applies
them on the game thread and replies; `cyrus-mcp` supplies the agent adapter.
This crate has no Bevy dependency.

In `Menu`, `state.menu` reports the page, installed entries, current selection,
preferences, and Controls capture action. Wait for `ready` before sending menu
input and after each animated page change. Quit closes the book before exiting.
Drive Options and Movies with `input`; the same saved bindings and values
apply after a native restart. Native Movies playback uses the saved subtitle and
interlace preferences and reports progress in `state.menu.movie`. A new key press
advances to the next installed skip point, then returns to the menu. Saved Sound
preferences apply to native effects, speech, and CD music. Browser Movies shares
playback rules using locally exported cinematic assets; it has no control port.

Acceptance-run commands for the script world (all need `scene_ready`; `scripts` pages at most 64 actors,
use `offset`):

- `scripts {offset?, limit?}` lists logical script actors with their id, attributes, control record,
  animation group and position (Q8; divide by 256 for map units).
- `place_player {x,y,z,heading}` moves the selected player to a map position and heading and clears its
  ledge, fall, slide and queued-motion state. It changes the logical player; `teleport` moves only the
  presentation. A script that re-places the player each loop (a teleporter pad's `MovePlayerAboveMe`) can
  undo it.
- `set_attribute {index,attribute,value}` writes one attribute byte of the actor at `index`; health is
  attribute 3 (`attr_health`). `mark_dead {index}` sets health zero and lifecycle selector eight, which is what `IsDead`
  reads, without the death animation.
- `cheat {name,value}` and `console {line}` run the installed cheats and console commands, for example
  `item add,79`; `drevil` stops the player taking damage from hits and direct damage and keeps it alive at zero health. `neversaydie` makes `PlayerDistance` return
  9999999, so every script gated on the player being near stays idle: leave it off for interaction runs.
- `select_item {id}` makes inventory item `id` the active item, as confirming it in the inventory screen does.
  `set_torch {burning}` sets the selected player's burning-torch counter (zero is unlit) without the lighting
  animation, to reach `TorchActivate` and other burning-torch gates.
- `session_slots {operation}` creates, lists, loads or deletes save slots (`create` needs the player idle);
  poll `session_slots_result` and send it again with `acknowledge: true` to clear a stale result.

`set_sky_scroll {direction, offset}` applies a captured original sky direction
from 0 through 2047 and two texture offsets in texels from -256 through 256.
Use it after loading the reference world. It synchronizes sky motion for
comparison; it does not synchronize other users of the original random stream.

`scripts {offset, limit}` returns bounded logical actor state. `step_script
{generation, index, stage, frame_scale, nominal_ticks}` selects `animation`,
`native_hook`, `entry`, `main`, `interrupt`, or `tasks`.
Native hooks dispatch the retained RAHK call without advancing either script continuation.
`tasks` requires an explicit unsigned `frame_scale` with eight fractional bits.
For example, 256 supplies scale one; zero is accepted. Main and interrupt body
steps do not poll existing tasks. These inspection tools do not enable gameplay.

`script_summary {offset?, limit?}` returns only actor identity, object id, health and prepared map
position. It avoids VM locals, task slots, geometry and failure serialization so native probes can
sample a world without blocking on the full `scripts` payload.

`bind_script_animation {generation, index, default_group, node?}` prepares an
actor once for this world generation. The caller supplies the established default
group. An optional exact scene node name transfers its animation presentation to
the logical actor. Without a node, stepping captures animation sound intents in `captured_sounds` instead of playing them. Immediate script sound commands retain their logical receiver, appear in `captured_sounds`, and reach the sound consumer when present. Once any actor is animation-bound,
all steps require explicit `nominal_ticks`, including zero, before execution.
These tools do not determine production eligibility or pacing.

Spatial inspection uses `step_script` stages `prepare_spatial`, `snapshot`,
`axis_locks`, `transform`, and `families`. Preparation is explicit and transactional. Snapshot
copies the selected actor's local position and angles; axis_locks restores currently
locked axes after script/movement work, except on slaves; transform refreshes an unattached
matrix; families composes prepared attachments. These operations grant no actor
eligibility. Once spatial state is prepared, executable stages need both
`frame_scale` and `nominal_ticks`, including zero.

Player queries use the current `PlayerPose`, not the inspection camera.
`distance_override` optionally overrides runtime `neversaydie` state;
`fallback_position` supplies original Q8 coordinates when no player exists.
Missing required inputs fail. Attached queries read the last composed pose.
Unattached linked-position selection remains unsupported in the script host.

The `ordinary_main` stage runs entry/tasks/main for an explicitly admitted
ordinary map actor and requires `frame_scale`. It honors script-disable and
task-bypass flags; separate `entry`, `tasks`, and `main` remain direct inspection
stages. It does not perform hook, combat, movement, or automatic eligibility work.

`cheat {name, value?}` applies a named toggle or explicit unsigned integer value.
Without a value, `oracle` lists state and `savecheats` writes the complete CHT file.
`state.cheats` reports readiness, load errors, raw values and each effect's support.
Unsupported effects retain their values but do not change gameplay. This is an
inspection interface; the original F12 console and gameplay typing remain pending.

`hold_texture_frame {world, image, frame}` holds a zero-based texture frame for
matched captures. `frame: null` resumes automatic requests from the selected
frame without replacing its deadline. `state.textures` reports loaded image
names, frame counts, selected frames and holds. The requested world must be
ready, and the image/frame must exist. World replacement clears holds. These
inspection controls do not change ordinary playback or establish cadence parity.

`bound_light_image {name, primitive}` selects an exact, unique Bevy node `Name`
and a zero-based material-bearing child in its current `Children` order, then
replies in `PostUpdate`, after ordinary material binding. Missing or duplicate
names, staged entities, mismatched prepared ownership and unavailable image bytes
return errors. The result contains the acquisition `frame`, `name`, `primitive` and
`bound_light_image` record with runtime entity/material/image identities,
world/generation/environment, dimensions and signed float rows after the 256
lookup rows. The response preserves the control request ID. It reads the actual
bound CPU asset and does not establish GPU sampling, mesh-generation ownership,
retired-owner absence or visual parity. Record matching camera, package/source
and screenshot identities separately.

The selector index refers to the current runtime hierarchy, not an original face
or material ID. A named material-bearing mesh itself accepts only index zero.
Out-of-range indices fail. Authored placement node names and generated primitive
names differ; global primitive names need not be unique across placed instances.

`rendered_actor {index, after_generation?}` observes a current actor after
presentation and transform updates. It reports logical/displayed model frames,
validated model source identity, and expected versus bound attachment sources,
owners, transforms and visibility. With `after_generation`, it waits for the
first settled presentation update of a replacement generation (up to 180 seconds).
Empty-world, scene preparation, retained revisit restoration and model-instance/mesh
loading keep the observation pending.
`state.rendered_actor_pending` confirms that the observation is armed before
starting a public load or revisit. One observation may be outstanding. This
read-only diagnostic does not reconstruct missing roots or advance animation.

`schedule_input {events}` delivers diagnostic input before ordinary player input
sampling. Each event has `after_updates` relative to the next admitted ordinary
update, starting at zero, and optional `key` and `action`. Supply both for an
explicit `press` or `release`, or neither to observe a boundary without changing
input. For example:

```json
{"id":2,"cmd":{"type":"schedule_input","events":[{"after_updates":0,"key":"ArrowUp","action":"press"},{"after_updates":12,"key":"ArrowUp","action":"release"}]}}
```

The deferred response contains `boundaries`, each with the actual `update`, world
`generation`, cumulative `game_seconds`, transition and selected logical actor
snapshot in `player.actors`. Snapshots precede input sampling and simulation at
that boundary. `game_seconds` excludes the sampled forthcoming step, so the
interval between press and release includes exactly the updates that held input.
Update counts do
not prescribe elapsed seconds. Compare the reported clocks before interpreting
motion or contact.

Schedules require a current admitted world, at most 256 ordered events, initially
released scheduled keys, and balanced explicit transitions. Taps are unavailable.
One schedule can run at a time; immediate control `input` requests are rejected
while it owns delivery. A lost admission, changed generation, skipped/repeated
update or 35-second execution deadline rejects the attempt and releases its held
keys. Unrelated held keys are preserved. Ordinary gameplay input sampling is
unchanged when no schedule is pending. This diagnostic facility does not pause,
step or change the simulation clock.

`effect_geometry {after_generation?}` observes transient billboard bindings after
PostUpdate. Without a generation it replies once; with one it waits up to 180
seconds for a different ready generation, completed dormant-world restoration
and its model-ready admitted clock sample, then observes that update without retrying missing geometry. Other commands remain
available while armed. `state.effect_geometry_pending` acknowledges admission, so
a driver can submit a load after the probe is armed. The result includes its
acquisition frame and CPU geometry/material/image observations. This does not
establish GPU submission or original pixel parity.

`capture_gameplay_frame {path, pcm_path?}` queues one native primary-window PNG.
The ready world, simulation clock, selected actor and player presentation are
frozen after transform publication, before render extraction. GPU readback and
PNG encoding finish asynchronously; reply delivery does not supply the capture
time. A second pending request, unavailable presentation, existing output or
30-second deadline fails explicitly. Only one encoder may remain active, even
after a timeout.

The optional PCM path is a stereo signed-16 recorder file. Its delivery-frame
cursor is latched with the render boundary. It is not a mixer-generation
timestamp; `exact_pcm_event_mapping` remains false. `render_frame` identifies
the extraction frame, while `update` identifies the ordinary simulation update.

`capture_gameplay_frame` accepts an optional `boundary`: either
`{"type":"clock","generation":1,"elapsed_scale":384}` (absolute consumed Q8
nominal ticks), or `{"type":"wall_contact","generation":1,"after_update":10}`.
The latter retains the first admitted selected-player primary wall response after
that ordinary update, including responses which suppress rollback while sliding.
It does not label floor support or another actor's query as player wall contact.
The capture waits at render extraction without changing simulation cadence;
a skipped clock, missed contact update, changed generation or deadline fails.
Omitting the boundary preserves next-frame capture. PCM delivery cursors remain
explicitly distinct from mixer generation attribution.

An opt-in Linux mixer tap supplies an additional `pcm_generation` receipt when
`CYRUS_MIXER_CAPTURE_DIR` names an empty output directory. It contains the last
completed callback's frame prefix, sequence, monotonic nanoseconds, rate and raw
sample format. These fields describe generation before audio-device submission,
independently of the optional monitor delivery cursor. A missing or invalidated
tap returns null. Producer discontinuities and recorder pacing remain separate
gates; neither receipt alone establishes device playback or original PCM parity.

`acquire_input {key, endpoints, normal_frame_rate, max_updates}` presses a released key at the next
admitted ordinary-update boundary. `endpoints` is a strictly increasing list of
positive elapsed Q8 nominal ticks from retained counterpart receipts, with at most
256 entries. The owner observes every ordinary boundary and releases at the first
endpoint actually consumed. It reports every boundary, including the press and
release, with actual update offsets, consumed scale and installed normal frame rate.
Changing cadence requires no predicted update count. Skipping every endpoint,
reaching the 1..255 update bound, losing the world or changing clock units rejects
the acquisition and releases the owned key. No timestep is changed or interpolated.
The caller must validate counterpart receipt continuity and matching clock units;
this command admits input timing, not placement, trajectory or rendering parity.
