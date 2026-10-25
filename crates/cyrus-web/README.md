# cyrus-web

The browser game runs startup through the book menu by default. It uses the same
world, game and presentation plugins as the native game. This host owns URL
selection and localStorage preferences; it has no native control port.

Preference storage preserves the accepted value when a write exceeds the browser
quota, and distinguishes missing records from read failures. Read/write errors
identify the document key and retain the browser's reason, including denied access
to local storage. Failed asset-source opens terminate their worker before returning
the original error.
Complete gameplay save restoration remains open.

With `?capture`, diagnostic session requests use `window.__cyrusSessionRequest`
as a JSON string. Send `{"type":"save","id":1,"key":"slot"}` or
`{"type":"load","id":2,"key":"slot"}`, then poll with
`{"type":"result","id":3}` and read `window.__cyrusSessionResult`.
Only one operation or unread result is accepted at a time. Saves use the
`cyrus-sessions` IndexedDB database; preferences retain localStorage. Save transactions
request strict IndexedDB durability and report `stored` only after completion. A load reports `committed` only after shared
source validation and exclusive live replacement. Malformed documents, missing
records and storage failures return contextual errors. These inspection requests
do not enable menu save slots or establish complete gameplay restoration.
The same diagnostic bridge accepts `{"type":"slots","id":4,"operation":{"type":"list"}}`.
Slot operations are `list`, `create` with `description`, `overwrite` with
`slot`, `revision` and `description`, and `load` or `delete` with `slot` and
`revision`. Poll `result` as above. Creation and overwrite capture the current
session. Load uses the same source preparation and exclusive commit as named loads.
The separate `cyrus-save-slots` database publishes metadata and complete session
text in one strict transaction. Listing reads metadata only, newest timestamp
first with ascending slot numbers on ties. Creation allocates the first free
manual slot from 2 through 299 inside the write transaction. Revision tokens reject
stale overwrite, load and delete requests. Descriptions support 1–64 original input characters, including byte codes 11
and 13 alongside printable ASCII. Other text is explicitly unsupported. Named diagnostic
snapshots are unchanged. These operations do not provide the book's dialogs,
thumbnails, quick saves, autosaves or complete gameplay save eligibility.
Retained-save death confirmation can load an existing slot through this host
without capture mode. Missing slots, stale revisions and failed restoration
report errors and retain the live session. Browser slot diagnostics retain an admitted
save selection before capture and a load selection before document decoding.
Save preparation reads metadata without writing; publication rejects a changed
planned slot or stale overwrite revision. The authored Load page also uses this host without capture mode. It lists the
first nine saves and restores the selected revision. The first Save page also uses this host for description editing and create/overwrite.
Additional list pages and thumbnails remain unavailable. Delete on an existing Load row asks
for confirmation, removes the selected revision and refreshes the list.
Run `testSessionStorage()` and `testSessionSlots()` from
`dev/scripts/test-session-storage.mjs` in a browser with real IndexedDB to verify
atomic publication, concurrent allocation, stale revisions and failed-write protection.
Browser load preparation resolves each saved world ID only after its decoded
script, animation and player sources agree. Rendered sources still require exact
compatibility with the saved document.

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
| `?new-game=true` | `--new-game` |
| `?movie=2` | `--movie 2` |
| `?skip-movies=true` | `--skip-movies` |
| `?water=modern` | `--water modern` |
| `?fog=modern` | `--fog modern` |
| `?sky=modern` | `--sky modern` |

Combine parameters with `&`; percent-encode values. Scene accepts a map stem or
WORLD.INI ID. Modes accept `faithful` and `modern`; defaults remain Faithful.
Booleans `skip-startup`, `new-game` and `skip-movies` accept an empty value, `true`/`1`, and `false`/`0`.
`view` supplies a one-shot inspection camera pose in scene coordinates with a
unit quaternion, including world entry through the menu. It does not move the player.
In a world, the world picker prepares the destination within the current session
and updates the URL without reloading the page. It clears the one-shot inspection
pose and preserves the install URL and other launch options. From startup or the
menu, the picker still navigates to a direct scene launch.
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
