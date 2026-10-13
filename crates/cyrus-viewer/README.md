# cyrus-viewer

A separate Bevy and egui application for inspecting your original Redguard
assets. Buy the game from GOG and point the native viewer at your own install.
The viewer reads the installation without modifying it.

The browser viewer shares the game's local folder picker. Choose the outer GOG
folder containing `Redguard`, `DOSBOX`, `game.gog` and `game.ins`; selected files
stay on your computer. No upload or install server is needed. Use `?install=local`
to override a saved service URL, and select the folder again on each page load.

```sh
cargo run -p cyrus-viewer -- --install /path/to/Redguard
# REDGUARD_DIR or default local/GOG folders can supply the location.
```

Prebuilt archives run with `./cyrus-viewer --install /path/to/Redguard` or
`cyrus-viewer.exe --install C:\path\to\Redguard` on Windows.
The Linux archive uses Ubuntu 24.04 system libraries.

If no usable install is found, enter its folder in the shared native picker or
choose Browse, then Continue. The picker accepts outer GOG folders and install
subfolders. It remembers the resolved location in `cyrus-config/install.json`,
shared with the game, and asks again when that location stops working.
Continue uses the existing application window and starts catalog loading
without launching another process. Cancel
closes the application without changing the saved location. Linux Browse needs
a desktop file portal or Zenity; entering a path also works without either.

Choose a file format, search by name or source path, and select an item. RGM, WLD,
ROB, 3D, and 3DC have separate categories. Numbered TEXBSI banks share one
category; derived World previews and CD audio have their own entries.
Up/Down changes selection when a text field is not focused. The Details panel
shows source information and links to related archive members, textures, and
world variants. Panels resize by dragging their boundaries.

Images support fit, zoom, frame selection, and palette selection for indexed
sources. Animated images and models have Play/Pause, Loop, and an editable frame
interval in milliseconds. Fonts show a sample and glyph atlas. Models and worlds have
Orbit (default) and Flycam camera modes, Textured, Unlit, Solid and Wireframe
shading, an optional edge overlay, frame controls where supported, and automatic
framing. Orbit uses drag to rotate, right-drag to pan, and scroll to zoom.
Flycam uses right-drag to look and WASD to move while the pointer is over the
viewport; Q/E moves down/up and Shift boosts speed fourfold. Flight speed scales
with the framed asset size, crossing half the framing distance per second.
Loading shows a progress bar, including archive indexing, preview preparation,
model dependencies and media. Stages without a measurable total use a spinner.
The stage label appears below the indicator.
Models start with the first available scene palette. The palette selector lists
palettes from their referencing worlds, including time-of-day variants, with
Without palette last. Indexed images use their embedded palette first when present; otherwise they
default to the first associated palette. Their selector and related-asset links use
terrain/model references or the matching sky palette. Without palette shows grayscale indices. Standalone model
references are indexed from scene placement and actor metadata on first use,
then reused; this does not run during startup.

Archive categories initially list archives. Open one to read its member index,
then select a member to load its contents. A BSI bank with one member opens that
image directly after indexing. Search covers files and the currently
opened archive. Speech entries use original IDs; labels become available after
selection.

Audio shows a waveform with playback, seeking, looping, and volume. Movies have
video, audio, a timeline, and frame stepping. Definitions offer readable source and JSON views alongside preservation's decoded
fields. RGM world previews and WLD terrain geometry can switch to JSON. PVO files
show a 3D wireframe octree with depth selection and Orbit/Flycam controls, a
top-down slice with depth and height controls, and a JSON view.
Malformed members remain visible with a reason. Unrecognized files are hidden.
WAV lists standalone files; SFX, RTX, and CD audio contain additional sounds.
PCX and LBM show indexed images with their embedded palettes. ANM shows decoded
frames and initializes playback with its authored frame interval. The installed
`pointers.bmp` cursor sheet uses preservation's GXA decoder and shows every image
in a scrollable grid without animation controls; standard Windows BMP
images also remain supported.

WLD selections open the heightmap automatically. The Texture rotations layer includes
a color legend for 0°, 90°, 180° and 270°; Tile directions adds arrows on the 3D terrain.
Terrain maps show heights, materials, quarter-turn rotations and wave-selected
vertices. Click a map cell or set its row and column to inspect its values and
highlight it in 3D. **Inferred:** direction arrows follow the tile's positive texture-U
axis, using preservation's terrain UV rotations.
**Verified:** WLD stores heights and texture rotations; terrain normals are derived.

Models offer rendered vertex and triangle normals, mesh bounds, authored subobject
spheres, a UV layout for the picked mesh and face picking. Normal spacing limits overlay density.
Picking reports the rendered triangle and normalized UVs. Authored polygon records,
normal tables, frame data and subobject face membership remain in JSON.

RGM inspection labels markers, lights and object placements in 3D. Navigation
graphs show directed routes in authored local X/Z coordinates. Actor inspection
connects placements, scripts, hooks, model references and animation commands.
Bounds use local XY/XZ projections. Collision sphere diagrams retain authored
radii, vertices and offsets without inventing their world poses. Light vectors
retain their documented direction/attenuation fields.

Select a PVO cell in the slice or cell list to highlight its static objects in
the associated world. **Verified:** PVO visibility IDs identify placed objects.
IDs beyond the static object table require runtime state and remain explicitly
unmapped. Configuration files retain source and JSON views.

Supported previews also offer JSON inspection. Images retain parsed headers,
frame descriptions, palettes and animation records; fonts retain glyph metrics;
models retain decoded geometry, normals, materials, frame records and authored bounds;
terrain retains every stored map plane plus decoded cell values; worlds retain
placement, navigation, actor and script records; audio retains
sample format and frame counts; movies retain dimensions and timing. Existing
archive indexes, spatial data, definitions and source views keep their decoded
JSON. Pixel, compressed-frame and PCM payloads are omitted from inspection JSON.
BSI texture palette choices follow referencing models and terrain through their
world assignments. Standalone sky images use their companion COL palette.
The References panel links the associated palette files and model/terrain sources.
Images without a known palette relationship offer embedded colors or raw indices.

TXT, MDI, DIG, PIF, OPL, AD, RST, DLL and EXE files, and the Miles driver-support file
`mssdrvr.lst`, are excluded from the catalog.
MDI, DIG, PIF, OPL, AD and RST are also excluded from preload parsing.

## Browser build

Build without game files, then serve your original install at runtime:

```sh
just viewer-web-build
just run-viewer-web --install /path/to/Redguard
```

For a prebuilt browser archive, use Python 3.11 or newer from its `browser/`
directory:

```sh
python3 viewer-server.py --directory . --install /path/to/Redguard --port 8080
```

Open `http://127.0.0.1:8080/viewer.html`. The server binds to loopback.

The browser worker parses the original files with the same Rust parsers as the
native viewer. Selecting an entry produces its preview and media in browser
memory. The server supplies only a file listing and original bytes. No preview
export is required.

The game and viewer share the browser bootstrap loader and service-worker cache.
The viewer keeps that loading overlay visible while its catalog starts; the game
uses its installed artwork for world loading.
Original downloads are cached by content digest. CD reads use byte ranges.
The selected asset is encoded as `#asset=<id>&file=<source-path>` in the browser URL. Copy that
URL to reopen the asset in the same installation; browser Back restores selection.
A selected model palette is also preserved in the URL.
Native cached previews remain on disk outside the install.

For private static hosting, serve `target/web`, the original files beneath
`original/`, and an `original/manifest.json` listing each file's `path`,
`bytes`, and 64-character content `digest`. The preview server's manifest
endpoint supplies this listing. The virtual paths `disc/game.gog`,
`disc/game.ins`, and `splash/glide2x_emu.ovl` refer to the associated GOG files.
The host must support HTTP byte ranges. Pass `?install=https://your-private-host/original/` or select that URL at startup.
The shared bootstrap validates and remembers it for both applications.

Original files and generated previews contain game data. Keep them private,
outside source control and application releases. Startup reads the file listing and small palette/configuration files. Archive
indexes are read only when opened. Older links without a source path may need
to scan archive indexes to locate their member.

## Current limits

The renderer is a generic inspection view. It does not claim Faithful game
lighting, animation timing, or scene behavior. Palette choices come from WORLD.INI and preservation's scene references. Without
a palette, models use embedded texture colors or grayscale palette indices.
Models with no known world reference have no scene palette choices. World
previews use their configured palette.
Model playback uses preservation's frame conversion and the chosen inspection
interval. It does not execute the original animation scripts. Unknown encodings
or incomplete frames report an explicit error. **Unknown:** the available BSI documentation does not establish separate mip-level
layouts. The viewer exposes decoded image frames and metadata without a mip selector.

Movie playback offers every authored audio track, with sample rate, channel count and
bit depth, or silent playback. Switching tracks preserves the timeline position. The selected movie is downloaded and
its audio prepared before playback; progress identifies reading and decoding.
Large movies still require the full original movie bytes. Seeking decodes forward
in bounded batches and may pause while catching up. Browser audio requires a user gesture.
Scripts are inspected as data, never executed.

`cyrus-assets` tests the portable catalog/runtime contracts. Native and browser
runtime checks are also needed to verify rendering, input, and media playback.

Native and browser selection options use exactly the same names:

| CLI | Browser URL |
| --- | --- |
| `--install PATH` | `?install=URL` |
| `--asset ID` | `?asset=ID` or `#asset=ID` |
| `--file SOURCE` | `?asset=ID&file=SOURCE` or the same fragment |
| `--palette NAME` | `?asset=ID&palette=NAME` or the same fragment |

URL values must be percent-encoded. `file` identifies an archive to resolve an
asset member; without it, older ID-only selections scan archive indexes.
`file` and `palette` require `asset`. Native selection waits for the catalog and
uses the same model palette rules as browser selection.

Without a supplied source, browser startup asks "Where are the game files?".
The validated URL is shared with the game in localStorage `cyrus-install` and
checked again at each launch. Unavailable files reopen the picker. The server's
`--install PATH` automatically
supplies the source to both pages; `--install-url URL` supplies a remote default.
AI-driven tests must always provide one of these sources explicitly.

World light inspection shows signed RGB strengths and authored radii. Light markers
use the exported scene origin; local lights are not presented as directional vectors.
