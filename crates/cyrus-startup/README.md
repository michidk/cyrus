# cyrus-startup

Installed splash/title presentation, the book menu and settings, cinematic playback,
world-entry loading, and the installed logbook reader. Book motion and helpers remain internal.
The book is a real 3D scene: its models are meshes under a perspective camera that renders into the
640 by 480 frame the book sprite shows, with the installed projection, placements and camera paths.
Page text, widgets and pictures are painted on the CPU into the page textures. F11 Flycam also flies
this camera in the book menu. While it is enabled the menu ignores the keyboard, unless F11's
"Pass input to game" is ticked. Dialogs are
drawn on a transparent layer over the scene. The logbook scrolls and the examine view are meshes
on the same kind of camera, drawn by one shared lit material that applies the original's per-channel
light levels (`view3d`). The 3dfx splash is also drawn live on the GPU every frame, from its
exported meshes and draws (`splash`).
Book motion includes the documented Save camera movement and reversible Load
page turn. Load opens the installed page and requests slot metadata from the native/browser
host. Its first page uses stored descriptions, retains slot revisions and loads
through the atomic session boundary. Empty rows are disabled. PageUp and PageDown
reach the remaining authored rows. Save opens the installed first page,
excluding reserved slots zero and one. New and existing rows open a 64-character description
editor and dispatch create or revision-checked overwrite through the session host.
New ASCII letters become uppercase; prefilled case is retained. Delete on an existing Load row
opens the installed mini-confirmation artwork; confirming deletes only the listed
revision and refreshes the rows.
The crate depends on assets/game/world/text/audio/media/app and Bevy; it does not
depend on cyrus-render. Preservation owns installed parsing and rendering facts.

Native and browser hosts add `StartupPlugin`. `StartupPresentation::without_splash`
skips the splash while retaining preparation. `StartGame` selects world-entry
loading presentation; `MenuState` exposes menu/control status and `QuitRequest`
asks the browser host to close its tab. Native hosts also add `LoadingPlugin`;
without startup presentation it retains headless parser-coverage preload.
Preparation results remain cyrus-assets types.

Native and browser debug world selections submit `WorldLoadRequest`. They use the
destination's installed `WORLD.INI` loading artwork, prepared on demand, with the
same progress and asset-readiness checks as New Game,
then recreate the player and enter at marker zero with heading zero. The world
camera pauses while the loading screen is visible. Selecting a world clears the
inspection camera and returns to the gameplay camera. Native direct-scene launches
also retain this loading lifecycle.

The menu initializes the shared game clock from `MenuAsset.clock` when absent.
Book motion and menu clock invocations follow changes in the host's BIOS-tick
sample. Menu exit restores the partial timing checkpoint while retaining menu
changes to smoothing, the measurement remainder, warmup and the last exposed step.
World timing remains game-owned; this does not enable automatic actor updates.
New Game applies the game-owned lockout reset before requesting its world. Loading a save does not apply that New Game reset.
World entry from the book discards the paused player before recreating it at the
requested marker, including when New Game selects the same world and marker.
The first-world fade waits for a published gameplay camera and consumes timing
from completed ordinary world updates. Host updates spent preparing assets do
not consume that fade. Loading-screen fades retain their presentation timing.
Startup, book, movie and loading presentation clear their borders to black
independently of the last world's background colour.

Startup copies title readiness and committed menu preferences, including subtitles,
into cyrus-audio before `ApplyPreferences`. Preferences still persist before they
apply. Keep `ApplyPreferences -> AnimationSystems -> PlayCues`; rendering hosts
own the optional listener pose. Cinematic entities carry audio's `CinematicAudio`
marker so CD music pauses during playback. Native media workers and browser
cooperative decoding retain their cancellation, seeking and menu-return behavior. Native video workers retain indexed delta-decoded frames and their palettes in a bounded queue. The audio-owned movie clock selects the due picture before tiled expansion; obsolete pictures do not accumulate raster work. Catch-up retains frame ordinals and the sample-exact seek origin.

Run `cargo test -p cyrus-startup` for startup, motion, raster, settings, movie and
loading contracts. Runtime checks require native/browser startup, menu, movie,
persistence and world-entry playtests with an explicit install. See
[SETUP](../../docs/SETUP.md) for commands and controls.

Default windowed startup plays the installed Windows Glide swoosh animation and decoded
640×480 title image. Native default launches then use installed startup policy
and validated native autosave state to select Introduction, book, or direct world
entry. The title remains visible while a pending launch action initializes;
book motion and drawing do not briefly appear before its cinematic. Explicit
`--skip-startup` retains book inspection. The browser uses the same installed
policy with its validated IndexedDB autosave before constructing the application.
Native menu/HUD export begins after the title becomes visible. The full
parser-coverage scan remains in headless inspection launches.
Loading stays asynchronous; skipping the splash retains required preparation.
The title uses the configured FNT font and documented bottom-label positions.
Label colours follow the installed title palette: the VGA-selected tint multiplies
the low font texture colour at palette index 161, following [preservation PR #313](https://github.com/michidk/redguard-preservation/pull/313).
The bottom-right red title label reports menu/HUD preparation, menu asset loading,
and browser sound loading. Labels describe cyrus stages rather than original map IDs.
**Verified:** the splash resource supplies 90 timed frames at 30 fps, followed by
the authored fade. The cache regenerates when the splash export changes. The [preservation contribution](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/GLIDE-SPLASH-DLL.md)
documents the DLL format. The splash draws at the physical resolution of the window's
fitted 4:3 viewport, including display scaling, and resizes with the window.
Its sprite retains the original logical framing without a fixed-resolution intermediate.
WebGL reconstructs screen-space colour interpolation from perspective varyings and
the fragment reciprocal W; native rendering uses the linear interpolation qualifier.
The book models use the same WebGL compatibility path for their lighting levels.
Glide edge coverage, floating-point approximations and pixel
quantization need a hardware comparison before claiming pixel-exact rendering.

The book menu derives entries, layout, texture slots, widgets, and action IDs
from `MENU.INI` through rgpre, following preservation's [book-menu rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md).
Startup entry uses preservation's context-one cover sweep and delayed opening cue.
Death context three uses its short entry path when the startup host has no retained
last-save selection. It suspends ordinary actor updates, disables world cameras,
and applies the selected actor override to Save/Continue availability. Returning
clears hit inhibition and the terminal outcome. Quit from a world context prepares
reserved slot one (`Redguard AutoSave`) before closing, or removes it while the death
override is active; Quit from the startup book does neither. Windowed direct-scene launches also install this service.
The shared `LastGameConfirmation` core applies the published Yes/No input ordering,
context-three entry delay and release-before-return rules. Its CPU painter restores
a supplied canonical capture for each redraw and uses installed MINIMENU artwork,
REDSEL, and SYSTEM.INI colour/alpha styles. Menu cache schema 17 includes those
assets and rejects older exports. Available-save death entry connects this core
to window capture and host-owned saved-game requests.
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
Scheduled source durations stop at the decoded-video boundary, including when a
single PCM packet spans several video frames. Extending that boundary schedules
only the new span and retains the unused packet tail. A delayed display update
cannot advance audio into video that has not decoded.
Packet starts and late-block offsets share the playback origin, so graph setup
work cannot shift continuation audio relative to the movie clock.
`node dev/scripts/test-movie-stream.mjs` tests this through the public movie API.
`node dev/scripts/test-browser-movie-clock.mjs CDP_URL PAGE_URL REFERENCE_WAV RESULT_JSON`
checks continuation timing and rendered PCM with a real WebAudio clock; supply
a secure page and a local WAV reference. It injects graph-connection work to
prove that continuation scheduling retains the sample origin.
Late PCM admission observes the exhausted boundary before extending lookahead,
so an unpolled producer stall cannot become skipped movie time. The public
`test-movie-stream.mjs` regression also covers seeking inside queued PCM and
cancelling obsolete source completion callbacks.
`node dev/scripts/test-browser-movie-seek.mjs CDP_URL PAGE_URL WINDOW_URL RESULT_JSON`
uses an installed decoded window from the assets `movie_window` example. It
presents those frames on the movie clock, admits a late continuation, pauses
and seeks with queued audio, and compares post-seek graph PCM with an offline
render of the scheduled sources and cancellations. This isolated adapter check
complements full native/browser cinematic comparisons.
Forward skips retain decoder state; menu return cancels downloads and playback.
Native and browser playback share skip points, subtitles and menu return.
MOVIE INTERLACE is saved but, as in the installed release, does not change the picture.
Movie frames use the preservation movie-raster contract: padded 256-pixel tiles,
clipped sampling coordinates, four-bit fractional filtering and dithered RGB565.
Subtitle glyphs use the selected eight-bit startup upload palette and integer
framebuffer blending. The complete 640 by 480 image is published atomically to
the GPU sprite; unused texture backing participates in filtering.
New Game plays installed movie zero (Introduction), then enters the installed
starting world and marker after playback completes or advances through its skip
points. Movie loading or playback failure returns to the menu with an error. Load restores the selected slot through the host session boundary. Shared world entry waits for preparation, required asset dependencies and
spawned scene instances behind the destination's installed GXA artwork, with preservation's
red bar and REDSEL percentage label. Progress counts completed loading stages.
**Verified:** original New Game loading shows the red bar and gold lower-left
percentage without a right-hand status message. Shared native/browser presentation
uses that layout and leaves the remaining artwork untouched. See preservation
[startup rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/startup.md#world-loading-screen).
The [preservation contribution](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/startup.md)
supplies the entry parser and loading layout. Save/Load uses the shared session
boundary and the native or browser slot host. Full story/gameplay initialization
and progression remain incomplete.

`MenuPreferences` stores original action units and fifteen binding codes in
native `menu.json` in the config directory or browser localStorage. It validates local state,
saves before applying edits, and leaves malformed files intact. Existing player
input uses edited bindings, including direct scene launches. The subtitle
setting applies to cinematics; saved Sound preferences apply to shared
effects and CD music, and native speech. Browser CD tracks use exported WAV files and
installed world assignments; native playback reads the CD image. Book motion and
direction/capture feedback dispatch installed effects through the shared native/browser sound path, following
preservation's [cue rules](https://github.com/michidk/redguard-preservation/pull/34).
**Verified:** the original default menu has no pointer hover/click selection; mouse/joystick
rebinding, lighting and camera-plane clipping remain unsupported. Escape in a world opens the book (`MenuContext::InGame`) only with SYSTEM.INI `final_version`. Entering the book from gameplay captures a
256x192 centre-cropped `GameplayThumbnail` for the book and for slot saves; leaving the book discards it. Entering the book from gameplay (pause or death) also stops every sound channel, as the original's book entry does; ambient loops restart from their scripts when play resumes. See [SETUP.md](../../docs/SETUP.md) for controls and limitations.

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
original never initialises, draws nothing but keeps its place in the layout.
Manual-entry presentation reads the game-owned text and cursor phase, restores the reader
frame, and draws the installed mini-menu artwork and REDSEL styles. Its display-copy
wrapping includes the underscore cursor and preserves CR/VT bytes without drawing symbols.
A CR advances the dialog text pen by one pixel, following the
[preserved spacing rule](https://github.com/michidk/redguard-preservation/pull/161).
`LogbookPlugin` registers the shared menu cache type and loader, including when used without
`StartupPlugin`; repeated registration preserves loaded menu assets.
The game owns all editing, submission, cancellation and persistence. The native and browser
hosts share the painter, which follows the
[preserved dialog rules](https://github.com/michidk/redguard-preservation/pull/159).
New Game deletes the stored journal; saved sessions retain accepted text,
duplicate-tag history and pending notification state through the game-owned journal snapshot.
The two scrolls are lit as the original's reader
lights them: its own ambient 64, a white directional light and two white local lights, independent of the world
(preservation's [scroll lighting](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#logbook-scroll-lighting)),
with corner levels interpolated across each face and the texture scaled by `floor(T * (L + 1) / 256)`; reader caches
written before this need re-export. The scroll textures take their colours from the paper file's own palette, as the original's
reader does. The 640x480 reader picture fills the window height at 4:3 and the GPU stretches it with bilinear filtering; a native run at 640x480 is
pixel-identical to the painted picture and one at 1024x768 matches that picture stretched by 1.6 (bilinear) within 0.3% of full scale.
The original's 16-bit framebuffer quantisation is not reproduced, which leaves the scrolls about two levels per channel brighter than
the original's. The browser host matches the native picture (a 640x480 CSS viewport at device scale 2 equals the picture stretched by 2 with
bilinear filtering within 0.3% of full scale, lettering and scroll lighting present). Rules follow the
[preservation contribution](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/hud.md#logbook-reading-screen).

Available-save death entry presents the installed last-game dialog over a captured
world frame. Its input and byte-colour composition use the shared confirmation
core. No enters the short death book; Yes submits the retained selection to the
host's `LastGameLoader`. Missing capture or load support produces an explicit
error. Native/browser rendered parity and successful restored-session acceptance
remain unverified; image resizing still requires comparison.

Introduction completion retains its documented entry heading through the loading request. Direct world entry and the no-movie shortcut retain heading zero.

Verified: F1 uses the installed mini-menu frame and font for the original version
information. Enter or the installed action key confirms, then waits for release.
F10 plays the installed credits directly and returns to the retained world on
completion or the release of Escape. Waiting for release keeps the return key
from also opening the gameplay book. Neither screen enters the book or plays the ending movie.
Both suspend simulation and rebase its clock before returning.
The behavior follows preservation's [fixed gameplay shortcuts](https://github.com/michidk/redguard-preservation/blob/c908f03bb1c67974bb83b24cecfa59b17bc9330b/docs/config/keys-ini.md#fixed-gameplay-shortcuts).

Audio synchronization applies committed local preferences before playing cues.
Opening the book preserves raw installed sound and music volumes; initializing
the displayed sliders does not write their rounded values back to the mixer.

The credits release lifecycle is documented in merged [preservation PR #319](https://github.com/michidk/redguard-preservation/pull/319).
**Verified:** credits Escape handling follows preservation's
[credits scroll](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/menu.md#credits-scroll).
Both F10 and ending credits retain the displayed frame and music owner while
Escape is held, then consume its release before returning. `CreditsExit` exposes
this shared input lifecycle; a movie-skip key held on entry cannot dismiss the credits.
