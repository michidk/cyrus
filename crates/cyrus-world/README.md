# cyrus-world

World lifecycle, actor animation playback and original actor matrix arithmetic.
It depends only on `cyrus-assets` (cached records) and `cyrus-soup` (typed
`AnimationGroup`); it needs no renderer, camera, audio backend, install parser or
sockets. Game, script, render, audio and the hosts build on it. SOUP execution
belongs to `cyrus-soup`, scripted hosts to `cyrus-script`, pose application to
`cyrus-render` and terrain/static collision to [cyrus-collision](../cyrus-collision/README.md).

## Modules

| Module | Responsibility |
| --- | --- |
| [lifecycle](src/lifecycle.rs) | `WorldPlugin`, `SceneRequest`, `PreparedWorld` (cache references), `CurrentScene`, `WorldChange`, `WorldMetadataStatus`, preparation progress |
| [animation](src/animation.rs) | `AnimationPlugin`, `AnimationSystems`, explicit `AnimationTick` playback |
| [animation/actor](src/animation/actor.rs) | `ActorAnimation`: requests, default group, priorities, checkpoints and versioned snapshots |
| [animation/playback](src/animation/playback.rs), [sequence](src/animation/sequence.rs), [class](src/animation/class.rs) | Command interpretation, frame sequences, per-type group rules |
| [sound](src/sound.rs) | `SoundCue` and `SoundSurface` intents; the audio consumer registers the queue |
| [transform](src/transform.rs) | `transform::Matrix` and the shared rotation table, with original single-precision and extended-intermediate arithmetic |
| [heading](src/heading.rs) | Heading arithmetic (2048 units per turn) |

## Notes

- Playback advances only on supplied `AnimationTick` messages (whole nominal
  ticks), never on render frames. No clock, actor activation or script scheduling
  is inferred here. Rules follow preservation's
  [animation facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/animation.md).
- Sound commands need a synchronous handler (`*_with_sounds`); without one, or on
  an unsupported cue, playback fails with a contextual error that stays sticky.
  See preservation's [sound facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md#animation-playsound).
- Neither world identity nor `WorldMetadataStatus` implies graphics or player
  sidecars are ready. `WorldPreparationPending` marks in-flight work;
  `PreparationProgress` is loading-bar history only.
- `ActorAnimationSnapshot` is an internal versioned format, not an original save
  record. Restoring it executes no commands or sounds; the host validates actor
  and world generation first. Bump the version when the layout changes.
- With `original-save-import`, `ActorAnimation::restore_original` binds the
  published save playback header to installed commands without executing them.
  A retained ShowFrame cursor restores the selected pose; unsupported cursors
  leave playback unchanged. `restore_original_bound` accepts independently
  validated selected/previous file-frame bindings and the stored default, so a
  non-ShowFrame cursor does not discard either pose. It validates the complete
  receiver before mutation and runs no commands. The host owns model allocations,
  the displayed pose and saved sound handles.
- Matrix arithmetic emulates the original's rounding in software and must not
  change host floating-point modes.
- Still unsupported: animated i16 frames, cross-model mesh replacement, automatic
  actor activation, complete script scheduling and a host-clock driver.

## Testing

```sh
cargo test -p cyrus-world
```

The tests in `tests/` cover animation, transitions, persistence, lifecycle and
transforms. All but the exported-model check in `tests/animation.rs` need no
install; that one reports a skip without it.
