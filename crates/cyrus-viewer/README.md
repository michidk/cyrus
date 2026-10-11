# cyrus-viewer

A separate Bevy and egui application for inspecting your original Redguard
assets. Buy the game from GOG and point the native viewer at your own install.
The viewer reads the installation without modifying it.

```sh
cargo run -p cyrus-viewer -- --install /path/to/Redguard
# REDGUARD_DIR can supply the same explicit location.
```

Prebuilt archives run with `./cyrus-viewer --install /path/to/Redguard` or
`cyrus-viewer.exe --install C:\path\to\Redguard` on Windows.
The Linux archive uses Ubuntu 24.04 system libraries.

Choose a file format, search by name or source path, and select an item. RGM, WLD,
ROB, 3D, and 3DC have separate categories. Numbered TEXBSI banks share one
category; derived World previews and CD audio have their own entries.
Up/Down changes selection when a text field is not focused. The Details panel
shows source information and links to related archive members, textures, and
world variants. Panels resize by dragging their boundaries.

Images support fit, zoom, animation-frame selection, and palette selection for
indexed sources. Fonts show a sample and glyph atlas. Models and worlds have a
textured orbit view, edge overlay, frame controls where supported, and automatic
framing. Drag to orbit, right-drag to pan, and scroll to zoom.
Models start without a scene palette. The palette selector lists only palettes
from their referencing worlds, including time-of-day variants. Standalone model
references are indexed from scene placement and actor metadata on first use,
then reused; this does not run during startup.

Archive categories initially list archives. Open one to read its member index,
then select a member to load its contents. Search covers files and the currently
opened archive. Speech entries use original IDs; labels become available after
selection.

Audio shows a waveform with playback, seeking, looping, and volume. Movies have
video, audio, a timeline, and frame stepping. Definitions expose preservation's
decoded fields and source lines. Malformed members of recognized asset types
remain visible with a reason. Unrecognized files are excluded from the viewer.

## Browser build

Build without game files, then serve your original install at runtime:

```sh
just viewer-web-build
just run-viewer-web /path/to/Redguard
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

The game and viewer share the loader, loading overlay, and service-worker cache.
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
The host must support HTTP byte ranges. The HTML `cyrus-assets` meta element
sets the original-file base URL.

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
Supported integer model frames can be scrubbed; other encodings show base
geometry with an explicit explanation. Preservation does not expose separate
BSI mip levels, so no mip selector is shown.

Movie playback uses the first audio track. Seeking decodes forward in bounded
batches and may pause while catching up. Browser audio requires a user gesture.
Scripts are inspected as data, never executed.

`cyrus-assets` tests the portable catalog/runtime contracts. Native and browser
runtime checks are also needed to verify rendering, input, and media playback.
