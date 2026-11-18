# cyrus-web

The browser build of the cyrus game. It runs the same world, game and presentation
plugins as the native game ([cyrus](../cyrus/README.md)), with
[cyrus-startup](../cyrus-startup/README.md) for splash, title, book menu and
cinematics. This host owns install selection, URL launch options and browser
storage. It has no control port; automated checks use the opt-in capture mode
below. It is not a default workspace member, so everyday builds and tests skip it.

## Play in the browser

Bring your own GOG copy of *The Elder Scrolls Adventures: Redguard*. The browser
build contains code only; your files are read in the page and never uploaded.

Click **Choose game folder** and select the outer GOG folder containing both
`DOSBOX` and `Redguard`, so the companion CD media is found too. Select the
folder again after every reload or after switching between game and viewer.
Click or press a key to enable audio.

To serve the files instead, run the bundled server from the extracted browser
archive (Python 3). `--install` is the inner folder containing `RGFX.EXE`:

```sh
python3 viewer-server.py --directory . --page index.html --port 8080 \
  --install /path/to/Redguard
```

Open <http://localhost:8080>, expand **Connect to a game-file server instead**
and click **Connect to server**. Stop the server with Ctrl+C. Any static host
serving a `manifest.json` and the original bytes works; cross-origin hosts need
CORS and byte-range support.

## Advanced launch options

Launch options use the native flag names as URL parameters (combine with `&`,
percent-encode values; booleans accept empty, `true`/`1` or `false`/`0`):

| URL parameter | Native flag | Meaning |
| --- | --- | --- |
| `?install=URL` | `--install PATH` | Game-file server; `?install=local` forces the folder picker |
| `?scene=START` | `--scene START` | Load a map stem or WORLD.INI ID directly |
| `?view=x,y,z,qx,qy,qz,qw` | `--view=…` | One-shot inspection camera pose for the first world |
| `?skip-startup` | `--skip-startup` | Skip the splash and open the book menu |
| `?new-game` | `--new-game` | Start New Game from the open book |
| `?movie=2` | `--movie 2` | Play a Movies-page cinematic (zero-based MENU.INI index) |
| `?skip-movies` | `--skip-movies` | Skip every movie except those opened from the Movies menu and the ending |
| `?volume=0` | `--volume 0` | Master volume for this visit, 0–100, not saved |
| `?mode=modern` | `--mode modern` | Every presentation area; `fog`, `sky` and `music` set one area |

Without launch parameters, the game opens the book after the splash and title, matching native startup. Choose New Game or load a save to enter a world.
Backtick opens the world picker, flycam and fidelity controls; F12 opens the
cheat console in a world. See [Playing](../../docs/PLAYING.md) for controls and saves.

Script world changes use the shared loading artwork and fade lifecycle; the URL follows the committed destination without reloading the page.

## Browser storage

Browser state stays in this site's storage for your browser profile:

| Key | Holds |
| --- | --- |
| localStorage `cyrus-install` | The last game-file server URL or `local` (shared with the viewer) |
| localStorage `cyrus-cheats`, `cyrus-console` | Cheat state and console preferences; installed files seed them when absent |
| IndexedDB `cyrus-save-slots` | Save slots, metadata and thumbnails |
| IndexedDB `cyrus-sessions` | Named diagnostic session snapshots |

## Modules

| Module | Responsibility |
| --- | --- |
| [main](src/main.rs) | App assembly, install source, URL options |
| [startup](src/startup.rs) | Book by default, with explicit New Game and movie shortcuts |
| [assets](src/assets.rs) | Asset reads waiting on original-file preparation in the worker |
| [session](src/session.rs) | Book and diagnostic save/load, slot storage and selection |
| [storage](src/storage.rs), [preferences](src/preferences.rs) | Document storage for preferences, cheats and saves |
| [cheats](src/cheats.rs), [console](src/console.rs) | CHT and CPF persistence, `CONSOLE.LOG` download |
| [bindings](src/bindings.rs), [debug](src/debug.rs) | Saved key bindings, world picker navigation |
| [capture](src/capture.rs) | Opt-in observation and diagnostic adapters (`?capture`) |
| [session-storage.js](session-storage.js) | IndexedDB transactions used by `session` |

## Notes

- Keep the URL table and the native flags identical: both parse through
  `cyrus_app::launch` ([cyrus-app](../cyrus-app/src/launch.rs)).
- Saves publish atomically with revision checks; a failed write or stale revision
  leaves the stored slot and the live session unchanged.
- AI-driven browser tests must pass the install on the server or in the URL so
  nobody has to answer the picker.

## Capture mode

`?capture` enables read-only observation and diagnostic request globals for
matched original/native/browser comparisons. They do not change gameplay and are
absent without the parameter. Each request is a JSON string; poll the matching
result and handle one request at a time. See [replay fixtures](../../dev/fixtures/README.md)
and [setup](../../docs/SETUP.md) for the workflows that use them.

| Global | Purpose |
| --- | --- |
| `__cyrusCapture` | World, camera pose, `ready`, `textures` and the frame `boundary`; `__cyrusInspectScripts`/`__cyrusInspectSession` add script actors and staging |
| `__cyrusStartup`, `__cyrusMenu` | Application state, startup presentation, game clock and book-menu selection |
| `__cyrusSkyScrollRequest` | Apply a measured sky scroll direction and offset |
| `__cyrusTextureFrameRequest` | Hold or release one texture animation frame |
| `__cyrusAcceptanceRequest` | `place_player`, `mark_dead`, `add_item`, `load_scene` setup helpers |
| `__cyrusSessionRequest` | Named session save/load and slot operations |
| `__cyrusAudioRequest`, `__cyrusSampleRequest`, `__cyrusVoiceStopRequest` | Start or stop installed speech and effects through the shared sound adapters |
| `__cyrusBoundLightImageRequest` | Read the CPU light image bound to one model primitive |
| `__cyrusFrameCaptureRequest` | Read back pixels at a game-clock or wall-contact boundary |

`dev/scripts/export-retained-frame.mjs` saves a retained frame capture as PNG.

## Testing

```sh
cargo test -p cyrus-web                         # host adapter tests, run natively
just web-build                                  # wasm build into target/web
just run-web --install /path/to/Redguard        # serve it with a read-only install
```

Installed browser acceptance drivers live in `dev/scripts/` and take a Chrome
DevTools URL, the page URL, an install URL and an output directory, for example
`node dev/scripts/test-browser-startup.mjs CDP PAGE INSTALL OUTPUT introduction`
(also `autosave`, `menu`), `test-browser-movie-handoff.mjs` and
`test-browser-world-entry.mjs`. Run `testSessionStorage()` and `testSessionSlots()`
from `dev/scripts/test-session-storage.mjs` in a browser with real IndexedDB.
The startup driver defaults to checking the book; `introduction` requests New Game explicitly.
The `listener_publication` example is an isolated camera/audio scene
(`cargo build -p cyrus-web --example listener_publication`).
