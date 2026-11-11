# Playing cyrus

cyrus is an unfinished rebuild. You can enter worlds, use the implemented controls,
and save and resume cyrus sessions. A complete playthrough and full fidelity to
Redguard have not been accepted. This guide describes current source; published
packages may lag recent fixes. See the [delivery issues](https://github.com/michidk/cyrus/issues/92)
for current support and [accepted differences](DIFFERENCES.md) for deliberate deviations.

## Start

Buy the GOG release of *The Elder Scrolls Adventures: Redguard* and install or
extract it. cyrus includes no game data. Download a native package from the
[releases](https://github.com/michidk/cyrus/releases), extract it and run `cyrus`.
Choose the inner `Redguard` folder containing `RGFX.EXE`, `maps` and `fxart`.
Keep the surrounding GOG folder intact so companion CD media remains available.
The install is read-only; cyrus keeps its own settings and saves elsewhere.

For the browser build, choose the outer GOG folder containing both `DOSBOX` and
`Redguard`. Select it again after reloading the page. Click or press a key in the
game to enable browser audio. A URL-based game-file server is also supported;
[Setup](SETUP.md#browser-build) explains it.

Choose **New Game** in the book menu. The introduction plays before the starting
world loads. Wait for loading to finish before using gameplay controls.

## Controls

**Verified:** gameplay bindings come from the installed `KEYS.INI` and your
saved Controls settings. The table gives the bindings in the GOG install used
for acceptance. Other installations or saved overrides can differ. Open
**Options → Controls** to inspect or change your actual bindings.

| Input | Action |
| --- | --- |
| Arrow Up / Down | Forward / backward |
| Arrow Left / Right | Left / right movement or turning, according to player state |
| Left Shift | Walk/run modifier; also the configured dialogue-skip input |
| Space | Jump |
| Alt | Look |
| Ctrl | Action, including activation or attack when admitted by player state |
| S | Quick sword |
| H | Quick health item |
| I | Inventory |
| Comma / period | Previous / next inventory item |
| L | Logbook |
| M | Island map |

A configured action does not establish that every movement, encounter or item
state is supported. Unfinished traversal and interaction remain tracked in the
[controller](https://github.com/michidk/cyrus/issues/65) and
[gameplay](https://github.com/michidk/cyrus/issues/4) issues.

**Verified:** the following shortcuts are handled by cyrus independently of the
editable gameplay bindings. Gameplay book shortcuts require the installed
`final_version` setting; the tested GOG install enables it.

| Input | Action |
| --- | --- |
| Escape, then release | Open the gameplay book; close or back out of other screens |
| F2 / F3 | Open Save / Load |
| F4 / F5 / F6 | Open Display / Sound / Controls |
| F7 | Toggle auto-defend |
| F8 | Cycle candle visibility |
| Tab | Toggle walk mode |
| F9 | Quick save, when saving is permitted |
| F12 | Open or close the developer console |
| F1 | Open cyrus inspection settings |

In the book, use Up/Down to select, Enter to activate, Left/Right to adjust an
option, and Escape to return. Wait for a page turn to settle. Mouse clicking
book entries is not implemented. Key rebinding starts when you activate a
Controls entry; Escape cancels capture. Browser or desktop shortcuts may
intercept function keys before the game receives them.

The fly camera is the bundled `flycam` mod (see `mods/README.md`): activate it in the
Mods window under F1, then F11 toggles it. WASD flies, the mouse looks and
Space/Left Shift moves vertically; the speed is a setting of the mod. The flight does
not move Cyrus through the world, and keys typed into the console are ignored.

Faithful controls are the default. `--controls modern` (browser
`?controls=modern`, or F1's Player controller setting) moves Cyrus with W, A, S
and D relative to the camera: he turns toward the held direction while moving
forward. He walks unless Shift is held, which runs; the Tab walk mode does not
apply. Space jumps as installed. With a gamepad, the left stick moves him the
same way: a partial tilt walks, a full tilt runs. The arrow keys and the D-pad do
not move him, and a quick sword key on W, A, S or D (S in the GOG install) moves
instead of drawing the sword. In conversation topic menus, W and S or the left stick
move the selection up and down. In the book menu, the D-pad or left stick acts as
the arrow keys, South (A on Xbox pads) as Enter and East (B) as Escape. `--camera modern` (`?camera=modern`, or F1's Camera setting) turns the
follow camera with horizontal mouse movement or the right stick and locks the
cursor during play; in a browser, click the game to lock it, also after Escape released it. Both sticks use the installed joystick dead zone.
Vertical mouse movement does not tilt the camera. The Faithful camera follows
Cyrus's heading, so with it A or D keeps turning him in a circle; use both Modern
settings together.

## Save and resume

**Verified:** native and browser builds support cyrus save slots with gameplay
thumbnails. While alive in a resumable world, press F2 or open the book and
choose **Save**. Choose a new slot, enter a description and confirm it. Selecting
an existing save edits its description and overwrites that slot. The Save page
reports errors; wait for completion before quitting.

Press F3 or choose **Load**, select the saved entry and confirm it. Loading
restores the accepted session and prepares its required worlds before returning
to gameplay. To check a save, quit or reload, reconnect the same install, then
load that entry. **Continue** resumes the world already held by the open book.

F9 replaces the reserved **Quick Save Game** entry and has no gameplay thumbnail.
Quitting through the book from a live world writes **Redguard AutoSave**; quitting
from the death book removes that automatic save. Manual saves remain separate.
Saving can be unavailable while the game cannot capture a resumable state.

Native saves live under `slots/` in cyrus's config directory. On Linux the default
is `~/.config/cyrus`; `CYRUS_CONFIG_DIR` overrides it, and an existing local
`game-config/` directory takes precedence over platform defaults. Back up the
whole config directory with the game stopped.

Browser saves use IndexedDB for the current site and browser profile. Reopen the
same address and profile to find them. Private browsing, clearing site data, or
changing the server's hostname or port can remove or hide those saves. Selecting
the game folder again reconnects assets; it does not import saves from the install.

**Verified:** original Redguard `.SAV` and `.TSG` gameplay saves cannot be imported.
cyrus saves are a separate format. The [compatibility investigation](https://github.com/michidk/cyrus/issues/93)
documents the unsupported original-save scope.

## Current limitations

- Full-game progression, alternate quest order and a new-game-to-ending
  playthrough remain unaccepted. Working startup, combat or save checks do not
  establish that the game can be finished.
- Movement, collision, follow camera, climbing and swimming still have open
  fidelity work. Remaining mouse-bound actions are incomplete; a gamepad drives the in-game controls and can be captured on the Controls page, but does not navigate the menus.
- Rendering, lighting, HUD, logbook, audio and movie synchronization still have
  open comparisons and defects. Faithful is the default; optional Modern settings
  do not imply that every proposed Modern control is implemented.
- Many developer commands and cheat effects remain unavailable. An accepted
  cheat value does not imply its gameplay effect works; read the console result.
- Linux and web have bounded clean-install and save/resume acceptance. Package
  availability on other platforms is not a claim of tested hardware support.
  Software rendering can be slow. Physical-GPU performance and audible output
  were not established by the headless release checks.

Report a problem with your cyrus version, platform, install source, scene,
steps and visible error in [GitHub issues](https://github.com/michidk/cyrus/issues).
Do not attach game assets, original saves or other copyrighted game data publicly.
