# cyrus-web

The browser game runs startup through the book menu by default. It uses the same
world, game and presentation plugins as the native game. This host owns URL
selection and localStorage preferences; it has no native control port.

Shared document storage preserves the accepted value when a write exceeds the
browser quota, and distinguishes missing records from read failures. Complete
gameplay save restoration and capacity for visited-world state remain open.
Read/write errors identify the document key and retain the browser's reason,
including denied access to local storage.

```sh
just web-build
just run-web --install /path/to/Redguard
```

The server validates the read-only install and provides its URL to both the game
and viewer automatically. The WASM worker prepares startup and requested worlds
from those original bytes in memory. Browser builds
contain code only and do not link the local install or its exports into the web root.
For an extracted browser archive, run:

```sh
python3 viewer-server.py --directory . --page index.html --port 8080 \
  --install /path/to/Redguard
```

Use the folder containing `RGFX.EXE` for `--install`. Open
<http://localhost:8080>, expand the server option and click **Connect to server**.
Stop the server with Ctrl+C.

Startup always shows a local folder picker and an expandable install-service URL
field. All browsers use a directory file input. Choose the main GOG folder
containing the game, DOSBOX and disc files. Selected files remain local and are
never uploaded. The browser grants access only to that folder and its descendants.
Select the folder again after reload or navigation between game and viewer.

The picker reports cancellation, validation failures and application preparation.
Companion files are discovered throughout the selected folder, including nested
GOG support directories. Local selection remembers only the preference `local`,
not the files.

URL `install`, the server default and the saved URL prefill the server field in
that order; none starts loading automatically. `?install=local` leaves it empty.
Click Connect to server to validate and load that source. A validated URL is saved
in localStorage `cyrus-install`, shared with the viewer. Cross-origin services
require CORS and byte-range support.
A static original-file host with a manifest supports both game and viewer; no
native exporter is required.

| Browser URL | Native CLI |
| --- | --- |
| `?install=URL` | `--install PATH` |
| `?scene=START` | `--scene START` |
| `?view=x,y,z,qx,qy,qz,qw` | `--view=x,y,z,qx,qy,qz,qw` |
| `?skip-startup=true` | `--skip-startup` |
| `?water=modern` | `--water modern` |
| `?fog=modern` | `--fog modern` |
| `?sky=modern` | `--sky modern` |

Combine parameters with `&`; percent-encode values. Scene accepts a map stem or
WORLD.INI ID. Modes accept `faithful` and `modern`; defaults remain Faithful.
Boolean `skip-startup` accepts an empty value, `true`/`1`, and `false`/`0`.
`view` supplies a one-shot inspection camera pose in scene coordinates with a
unit quaternion, including world entry through the menu. It does not move the player.
World-picker navigation clears the one-shot inspection pose and preserves the install URL and other launch options.
AI-driven browser tests must always supply the install to the server or in the URL,
so the user never has to answer the prompt during testing.

WASM downloads before Bevy starts; selected-host game assets load behind installed
artwork. New Game and the temporary Load fallback wait for world dependencies.
Movies read compressed Smacker ranges and play incrementally decoded first-track PCM; subtitle, interlace,
skip and menu-return rules share the native implementation. Quit attempts tab
closure and keeps running when refused. F1 opens the world,
flycam and fidelity controls during startup, at the book menu, and in a world. Direct scenes apply exported keyboard defaults and
saved Controls settings. Gameplay and original-game comparison limits are unchanged.

Direct scene launches use the shared in-game presentation without an additional
HTML control hint. F1 opens the inspection controls.

Not a default workspace member so day-to-day builds and tests stay lean.

Browser audio activates on keyboard input or pointer release. Movies, menu effects and
CD music share native playback rules; `--export-startup` exports the required CD tracks
and installed assignments. Runtime exceptions and graphics context loss display an error
instead of leaving an unexplained black screen.

[cyrus-startup](../cyrus-startup/README.md) supplies splash, title, the book menu,
settings, cinematic playback and world-entry loading. Hosts add it alongside
cyrus-render and cyrus-audio.

Matched captures use `?capture=true` with `view` and either `scene` or menu entry. This enables the
read-only `window.__cyrusCapture` observation after world entry, containing the
requested world and live scene-space camera pose. It does not change simulation
or camera behavior. See [tracked fixtures](../../dev/fixtures/README.md) for the
original/native/browser replay workflow and local artifact rules.

F12 opens the shared cheat console while in a world. Cheat state restores from
localStorage `cyrus-cheats`, falling back to the read-only installed CHT only when
local state is absent. SYSTEM.INI startup defaults apply afterwards. `savecheats`
persists through the same browser storage helper as preferences. The F1 debug
menu's Cheat files section imports or downloads original-format CHT files.
Import validates and saves before replacing live state; cancellation, invalid
files and storage failures preserve the current state. Import also recovers from
malformed local state. Clear the storage key to restore the installed seed.

Capture observation `window.__cyrusCapture.ready` reports that the current rendered world has finished spawning. The replay waits for it before measuring and capturing the view.

Console preferences use localStorage `cyrus-console`, retaining CPF bytes through
rgpre. The optional installed CPF supplies defaults only when local state is
absent. `console save` persists current preferences; `console write` downloads
`CONSOLE.LOG`. Malformed stored CPF data blocks edits and saves and is reported
by `console show`.

In capture mode, freeze the sky with the original `fxskyspeed 0` console command,
then assign a JSON string to `window.__cyrusSkyScrollRequest`:

```js
window.__cyrusSkyScrollRequest = JSON.stringify({
  id: 1, world: window.__cyrusCapture.world, direction: 0, offset: [0, 0]
});
```

Use the measured original direction and texel offsets for a comparison. The
request is consumed once while InGame. `window.__cyrusSkyScrollResult` reports
`id`, `accepted` and `error`. Acceptance queues the phase for the next sky update;
wait for a subsequent rendered frame before capturing. A mismatched or unready
world, malformed request or invalid phase is rejected without changing the sky.
Submit one request at a time and check its result. This inspection control is
absent without `capture`; it does not reproduce original-session random order.

The browser host selects `ViewCamera` for `cyrus_audio::follow_listener` before
audio preferences and cue processing; rendering does not depend on audio.

Capture mode also reports shared texture state in `window.__cyrusCapture.textures`.
Set `window.__cyrusTextureFrameRequest` to a JSON string containing `id`, `world`,
`image` and a zero-based `frame` to hold a measured frame. Read
`window.__cyrusTextureFrameResult` for acceptance or an error; use `frame: null`
to resume automatic requests. The world must be ready and match the request.
Unknown images and invalid frames leave playback unchanged. Holds clear on
world replacement and do not establish animation cadence parity.
