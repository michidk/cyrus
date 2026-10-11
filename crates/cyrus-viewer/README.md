# cyrus-viewer

A separate Bevy and egui application for inspecting your original Redguard
assets. Buy the game from GOG and point the native viewer at your own install.
The viewer reads the installation without modifying it.

```sh
cargo run -p cyrus-viewer -- --install /path/to/Redguard
# REDGUARD_DIR can supply the same explicit location.
```

Choose a category, search by name, source path, or format, and select an item.
Up/Down changes selection when a text field is not focused. The Details panel
shows source information and links to related archive members, textures, and
world variants. Panels resize by dragging their boundaries.

Images support fit, zoom, animation-frame selection, and palette selection for
indexed sources. Fonts show a sample and glyph atlas. Models and worlds have a
textured orbit view, edge overlay, frame controls where supported, and automatic
framing. Drag to orbit, right-drag to pan, and scroll to zoom.

Audio shows a waveform with playback, seeking, looping, and volume. Movies have
video, audio, a timeline, and frame stepping. Definitions expose preservation's
decoded fields and source lines. Unsupported files and malformed members remain
visible with a reason.

## Browser build

Build without game files, then serve your original install at runtime:

```sh
just viewer-web-build
just run-viewer-web /path/to/Redguard
```

The browser worker parses the original files with the same Rust parsers as the
native viewer. Selecting an entry produces its preview and media in browser
memory. The server supplies only a file listing and original bytes. No preview
export is required.

The game and viewer share the loader, loading overlay, and service-worker cache.
Original downloads are cached by content digest. CD reads use byte ranges.
Native cached previews remain on disk outside the install.

For private static hosting, serve `target/web`, the original files beneath
`original/`, and an `original/manifest.json` listing each file's `path`,
`bytes`, and 64-character content `digest`. The preview server's manifest
endpoint supplies this listing. The virtual paths `disc/game.gog`,
`disc/game.ins`, and `splash/glide2x_emu.ovl` refer to the associated GOG files.
The host must support HTTP byte ranges. The HTML `cyrus-assets` meta element
sets the original-file base URL.

Original files and generated previews contain game data. Keep them private,
outside source control and application releases. Initial browser indexing reads
archives to discover their members.

## Current limits

The renderer is a generic inspection view. It does not claim Faithful game
lighting, animation timing, or scene behavior. Standalone models use embedded
texture palettes where available; world previews use their WORLD.INI palette.
Supported integer model frames can be scrubbed; other encodings show base
geometry with an explicit explanation. Preservation does not expose separate
BSI mip levels, so no mip selector is shown.

Movie playback uses the first audio track. Seeking decodes forward in bounded
batches and may pause while catching up. Browser audio requires a user gesture.
Unknown formats remain listed. Scripts are inspected as data, never executed.

`cyrus-assets` tests the portable catalog/runtime contracts. Native and browser
runtime checks are also needed to verify rendering, input, and media playback.
