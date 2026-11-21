# cyrus-session

Shared capture, preparation and publication of a running game session for the
native (`cyrus`) and browser (`cyrus-web`) hosts. It snapshots the logical
registry (current and visited worlds), player, HUD, clock, RNG, cheats, audio
and CD playback, camera, sky, texture and terrain state, prepares a replacement
off-live, and publishes it in one exclusive commit. It depends on the state
owners (`cyrus-game`, `cyrus-script`, `cyrus-world`, `cyrus-audio`,
`cyrus-render`, `cyrus-assets`); none of them depends on this crate. It does no
I/O: hosts resolve installed assets and store the document.

For player-facing save and load behavior, see [Save and resume](../../docs/PLAYING.md#save-and-resume). This page describes the shared implementation and its boundaries. The document is a cyrus format, not the original game's save format.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `RuntimeSessionSnapshot`, `RuntimeSessionPlan`, `SessionServices` and `RuntimeSessionCandidate`: capture, plan, assemble |
| [document](src/document.rs) | `encode_document` / `decode_document`: versioned UTF-8 framing with a payload digest |
| [boundary](src/boundary.rs) | `SessionBoundaryPlugin`, `SessionBoundary` requests run in `Last`, read-only `SessionBoundaryInspection` |
| [loading](src/loading.rs) | `SessionLoadAssets` and frame-spanning hidden preparation before publication |
| [sources](src/sources.rs) | `SessionWorldAssets` and `SessionSourceCatalog`: host-resolved world sources for the saved census |
| [scene_instances](src/scene_instances.rs) | Hidden world/model instances, staging camera, sky, texture and material preparation |
| [entities](src/entities.rs), [staging](src/staging.rs) | Actor-backed entity census and fresh standalone targets with owned cleanup |
| [render_playback](src/render_playback.rs) | Texture and terrain continuation on the shared presentation epoch |
| [music](src/music.rs) | CD transport capture |
| [lifecycle](src/lifecycle.rs) | Settled world identity and prepared host references |
| [publication](src/publication.rs) | Exclusive commit of the prepared owners |

## Usage

Both hosts add `cyrus_session::SessionBoundaryPlugin`. A host then submits
requests on `SessionBoundary`:

- `capture_live()` or `capture(sources, hud)` snapshots the settled session;
  `commit(candidate)`, `load(...)` and `load_saved_game(...)` replace it.
- The request runs in an exclusive `Last` system after ordinary updates; read it
  with `take_result()`. An unread result blocks the next submission.
- Native hosts may enable `original-save-import` and submit `import_original`
  with a captured matching-world native baseline and validated original subset.
  The subset changes only the hidden candidate before normal publication. Saved
  continuations are unsupported; this API does not construct a fresh world.
- Encode a snapshot with `encode_document()`; `decode_document()` checks version
  and digest only. The decoded snapshot still goes through plan, assembly and the
  commit preflight.

Diagnostic hooks: native control `session_capture` then `session_capture_result`
on a later frame; in the browser with `?capture`, write a request to
`window.__cyrusSessionRequest` and read `window.__cyrusSessionResult` (see
[cyrus-web](../cyrus-web/README.md)). Neither writes a file.

## Boundaries

- Construction and preparation run no scripts, initialization, animation or
  playback. Replacement scene roots, cameras and entities stay hidden and
  privately owned until commit; `discard()` removes only what the stage owns.
- Commit reruns its preflight (actor generations, entity ownership, scene
  readiness, source content) before retiring anything. A rejected candidate
  leaves the live session untouched.
- Capture rejects anything it cannot continue exactly (pending sources, in-flight
  world preparation, unbound audio owners, inspection `ScriptedPlayback`) instead
  of guessing. Never rebind or synthesize state on load.
- Apply saved environment edits only to off-live copies
  (`RuntimeSessionSnapshot::prepare_environments`), never to live assets.
- HUD presentation is optional in a checkpoint. Loading one captured before the
  HUD initialized keeps the live overlay; logical HUD state still comes from the save.
  Prepared HUD presentation can also initialize an absent live overlay.
- A saved-game load follows the original's audio cleanup: samples and dialogue
  stop, the live CD transport is kept. See preservation's
  [sound](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/sound.md)
  and [music](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/music.md) docs.
- Audio snapshots cover source decoders, not downstream output queues; the digest
  detects corruption, not tampering. Hosts own input-size limits.

## Testing

```sh
cargo test -p cyrus-session
just verify session
```

The integration tests in [tests/runtime_session.rs](tests/runtime_session.rs)
build synthetic worlds and do not need an install.
