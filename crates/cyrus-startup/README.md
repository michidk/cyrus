# cyrus-startup

Installed splash/title presentation, the book menu and settings, cinematic playback,
world-entry loading, and the installed logbook reader. Book motion, rasterization and helpers remain internal.
Book motion includes the documented Save camera movement and reversible Load
page turn. Save-slot dialogs and activation remain unavailable; the temporary
fresh-world Load action is unchanged.
The crate depends on assets/game/world/text/audio/media/app and Bevy; it does not
depend on cyrus-render. Preservation owns installed parsing and rendering facts.

Native and browser hosts add `StartupPlugin`. `StartupPresentation::without_splash`
skips the splash while retaining preparation. `StartGame` selects world-entry
loading presentation; `MenuState` exposes menu/control status and `QuitRequest`
asks the browser host to close its tab. Native hosts also add `LoadingPlugin`;
without startup presentation it retains direct/headless parser-coverage preload.
Preparation results remain cyrus-assets types.

The menu initializes the shared game clock from `MenuAsset.clock` when absent.
Book motion and menu clock invocations follow changes in the host's BIOS-tick
sample. Menu exit restores the partial timing checkpoint while retaining menu
changes to smoothing, the measurement remainder, warmup and the last exposed step.
World timing remains game-owned; this does not enable automatic actor updates.
New Game applies the game-owned lockout reset before requesting its world. The
temporary fresh-world Load action does not apply that New Game reset.

Startup copies title readiness and committed menu preferences, including subtitles,
into cyrus-audio before `ApplyPreferences`. Preferences still persist before they
apply. Keep `ApplyPreferences -> AnimationSystems -> PlayCues`; rendering hosts
own the optional listener pose. Cinematic entities carry audio's `CinematicAudio`
marker so CD music pauses during playback. Native media workers and browser
cooperative decoding retain their cancellation, seeking and menu-return behavior.

Run `cargo test -p cyrus-startup` for startup, motion, raster, settings, movie and
loading contracts. Runtime checks require native/browser startup, menu, movie,
persistence and world-entry playtests with an explicit install. See
[SETUP](../../docs/SETUP.md) for commands and controls.

Default windowed startup plays the installed Windows Glide swoosh animation and decoded
640×480 title image, then opens the book menu once required assets are ready.
Native menu/HUD export begins after the title becomes visible. The full
parser-coverage scan remains in direct/headless inspection launches.
Loading stays asynchronous; skipping the splash retains required preparation.
The title uses the configured FNT font and documented bottom-label positions.
The bottom-right red title label reports menu/HUD preparation, menu asset loading,
and browser sound loading. Labels describe cyrus stages rather than original map IDs.
**Verified:** the splash resource supplies 90 timed frames at 30 fps, followed by
the authored fade. The cache regenerates when replacing the older DOS shield
splash. The [preservation contribution](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/GLIDE-SPLASH-DLL.md)
documents the driver variants and DLL format. Splash rasterization uses
supersampling; Glide edge coverage, floating-point approximations and pixel
quantization need a hardware comparison before claiming pixel-exact rendering.

The book menu derives entries, layout, texture slots, widgets, and action IDs
from `MENU.INI` through rgpre, following preservation's [book-menu rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md).
Startup entry uses preservation's context-one cover sweep and delayed opening cue.
Death context three uses its short entry path when the startup host has no retained
last-save selection. It suspends ordinary actor updates, disables world cameras,
and applies the selected actor override to Save/Continue availability. Returning
clears hit inhibition and the terminal outcome. Available-save confirmation and
loading and Quit automatic-save preparation remain explicitly unavailable;
direct-scene hosts do not install this service.
Its portable `cache/menu/book.menu` retains all authored model frames and topology,
textures, indexed fonts, previews, and defaults. Native and browser builds project
the current book pose and redraw neighboring-page text during camera entry, page
turns, and Quit closing. Input waits through the final settling update.
Navigation skips disabled/hidden entries and wraps. Options opens Display, Sound,
and Controls; Movies browses unlocked definitions and previews. Native Quit exits
after the authored close. Browser Quit requests tab closure and leaves the menu
running if the host refuses.
Native Movies playback reads installed Smacker files or the GOG CD image via
rgpre and decodes them in-process through `cyrus-media`. Playback starts from the
header and first packet, without reading the entire movie or decoding its full
audio track. Two video frames and four audio blocks bound decoded lookahead;
the audio callback never waits for disk reads or decoding. Audio starvation
outputs silence and freezes the sample clock without losing stereo alignment.
Dropping playback cancels the bounded readers and producers. It retains menu selection, consumes original
skip points and timed subtitle definitions, and exposes progress through menu
control state. Browser Movies reads installed compressed packets through the read-only install
service. cyrus-media decodes video and first-track PCM incrementally. A bounded
Web Audio adapter supplies the playback clock and pauses it while buffering.
Forward skips retain decoder state; menu return cancels downloads and playback.
Native and browser playback share skip points, subtitles, interlace and menu return.
New Game plays installed movie zero (Introduction), then enters the installed
starting world and marker after playback completes or advances through its skip
points. Movie loading or playback failure returns to the menu with an error. Load temporarily starts
the same fresh world. Shared world entry waits for preparation, required asset dependencies and
spawned scene instances behind the destination's installed GXA artwork, with preservation's
red bar and REDSEL percentage label. Progress counts completed loading stages.
**Verified:** original New Game loading shows the red bar and gold lower-left
percentage without a right-hand status message. Shared native/browser presentation
uses that layout and leaves the remaining artwork untouched. See preservation
[startup rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/startup.md#world-loading-screen).
The [preservation contribution](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/startup.md)
supplies the entry parser and loading layout. Saves, full story/gameplay
initialization and progression unlocks remain unavailable.

`MenuPreferences` stores original action units and fifteen binding codes in
native `cyrus-config/menu.json` or browser localStorage. It validates local state,
saves before applying edits, and leaves malformed files intact. Existing player
input uses edited bindings, including direct scene launches. Subtitle and interlace
settings apply to cinematics; saved Sound preferences apply to shared
effects and CD music, and native speech. Browser CD tracks use exported WAV files and
installed world assignments; native playback reads the CD image. Book motion and
direction/capture feedback dispatch installed effects through the shared native/browser sound path, following
preservation's [cue rules](https://github.com/michidk/redguard-preservation/pull/34).
**Verified:** the original default menu has no pointer hover/click selection; mouse/joystick
rebinding, lighting, camera-plane clipping and gameplay thumbnails remain unsupported. See [SETUP.md](../../docs/SETUP.md) for controls and limitations.

The in-game logbook reader loads `LOGPAPER.GXA`, small normal/selected fonts and
`SCROLL.3D` through shared asset preparation. The installed log binding opens
accepted local entries at the newest text. Installed up/down and A, Home/End and
Page Up/Down drive the shared navigation core. Log, Escape or installed D closes
it; gameplay resumes after those keys are released. The game owns the input lock
and simulation checkpoint. Existing audio continues. Native control state exposes
`logbook` readiness, scroll position and errors. Malformed stored entries remain
unchanged. The reader draws styles 0 to 4 and 6 to 9 of preservation's
[reader style table](https://github.com/michidk/redguard-preservation/pull/114) with its pixel
wrap and constant colour factor (styles 1 and 2 use page-palette colours); style 5, which the
original never initialises, draws nothing but keeps its place in the layout. Manual entry editing,
new-game resets and save integration remain unsupported. Scroll lighting is unsupported
(the original's light inputs are unknown) and pixel-exact filtering still needs fidelity work. Rules follow the
[preservation contribution](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#logbook-reading-screen).
