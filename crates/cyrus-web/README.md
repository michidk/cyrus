# cyrus-web

The browser game runs startup through the book menu by default. It uses the same
world, game and presentation plugins as the native game. This host owns URL
selection and localStorage preferences; it has no native control port.

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

Without a supplied or remembered URL, startup offers a local folder picker and
an install-service URL field. Choose the outer GOG folder containing `Redguard`,
`DOSBOX`, `game.gog` and `game.ins`. The browser reads selected files locally;
it does not upload them or require a file server. Chromium uses a read-only
directory permission; other browsers use a directory file input. Select the
folder again after reloading or navigating between game and viewer. Cancelling
leaves the picker open. `?install=local` opens this picker even with a saved URL
or server default. Local selection remembers only that preference, not the files.
The picker reports folder scan progress, cancellation and denied access, and shows
application preparation immediately after accepting a folder. Server connection
is available under an expandable option. Companion files are discovered throughout
the selected folder, including nested GOG support directories. Files outside that
folder are inaccessible; select the main installation folder when a companion is missing.

For HTTP sources, startup saves a validated install-service URL under localStorage `cyrus-install`, shared
with the viewer, and checks the original files at each launch. Missing files reopen
the picker. URL `install` overrides
the server default, which overrides the saved URL. Server `--install-url URL`
provides a remote default. Cross-origin services require CORS and byte-range support.
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
`view` requires `scene` and supplies a one-shot inspection camera pose in scene
coordinates with a unit quaternion. It does not move the player.
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

Not a default workspace member so day-to-day builds and tests stay lean.

Browser audio activates on keyboard input or pointer release. Movies, menu effects and
CD music share native playback rules; `--export-startup` exports the required CD tracks
and installed assignments. Runtime exceptions and graphics context loss display an error
instead of leaving an unexplained black screen.

[cyrus-startup](../cyrus-startup/README.md) supplies splash, title, the book menu,
settings, cinematic playback and world-entry loading. Hosts add it alongside
cyrus-render and cyrus-audio.

Matched captures use `?capture=true` with `scene` and `view`. This enables the
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
