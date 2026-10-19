# Asset viewer architecture

The maintainer approved this architecture. The native and browser applications
are implemented; the crate README records current controls and limits. The
viewer is a separate development application;
its interface and inspection controls are modern, independent of game fidelity.

The design priority is a useful asset foundation for the whole project.
Choose shared ownership before implementing previews, and refactor existing
callers where needed to establish it. Minimize unnecessary mechanisms, not the
work needed to make the architecture and asset organization coherent.

## Application boundary

Add `cyrus-viewer` with a native executable and a WebAssembly target using one
Bevy 0.20 and egui implementation. Native launches require `--install PATH` or
`REDGUARD_DIR`, validate that location, and read it without modification. Do not
assume a current-directory install. Both targets must compile without game data
and parse original assets at runtime. Browser launches fetch original files
from the configured game-asset source and reuse the shared cache. A dedicated
WASM worker runs the shared parsers and transfers preview/media bytes to the
viewer. The server serves only the original-file manifest and bytes.

Extract two shared responsibilities and migrate existing callers with them:

- `cyrus-assets` owns install validation, the asset catalog, portable preview
  descriptions, runtime parsing through rgpre, and cache publication.
  It has no game-state or rendering dependency. Filesystem and browser fetching
  belong at the input boundary; format parsing must work on both targets.
- `cyrus-app` owns common CLI argument definitions, Bevy window and asset-source
  setup, loading progress presentation, and egui initialization. It does not
  choose gameplay plugins or game-state transitions. Browser download/cache
  code and build scripts are shared by the two applications.

`cyrus-viewer` owns navigation, selection, inspectors, preview cameras, and
preview lifetime. Game settings, book-menu policy, player behavior, control
protocol, and script scheduling remain game responsibilities. Viewer settings
must not change game preferences. Keep the game's existing launch compatibility
while giving the viewer explicit install selection.

Reuse portable game rendering where practical. World previews may expose the
existing Faithful/Modern appearance controls. Standalone models initially use
textured Bevy rendering. A generic preview must not be labeled faithful, and
reusing Faithful rendering does not establish new original-game parity.
Extract a rendering crate only if this reuse needs that boundary; do not make
a full game-crate split a prerequisite. If renderer reuse would otherwise
duplicate rendering code or require installing gameplay systems, establish
`cyrus-render` as the shared owner and migrate both consumers. The criterion
is a sound dependency boundary, not the number of files changed.

The dependency direction is app entry points to application plugins, then
shared assets and presentation. Shared crates must not import `cyrus-game` or
`cyrus-viewer`. Export schemas and their readers have one owner; app plugins
select behavior through that contract. Prefer direct typed APIs over a dynamic
asset-provider framework or a configurable application framework.

## Browsing and coverage

Use resizable category navigation, a searchable asset/member list, a large
preview, and collapsible details. Preserve source names and expose format,
source path, member identity, context, and errors. Support keyboard selection
and prevent camera input while interacting with text fields or other controls.

Organize artwork by content kind, then source archive/file and member. Keep
frame, mip, palette and world variations attached to the asset they describe.
Let users follow a model's texture references or a world's models without
searching again. Missing references and ambiguous variants are visible rather
than resolved arbitrarily. Derive labels and membership from installed data.

| Content | Sources | Intended inspection |
| --- | --- | --- |
| Images | BSI/TEXBSI, GXA, title/splash assets | Zoom, transparency, frame and mip selection |
| Palettes | COL | Swatches, color values, indexed-image context |
| Fonts | FNT | Glyph atlas, metrics, sample text |
| Models | 3D, 3DC, ROB members | Textured orbit view, wireframe, supported frame scrubbing |
| Worlds | WORLD.INI, RGM, WLD | Terrain, placements, variants, linked assets, inspection camera |
| Audio | SFX, RTX, CD tracks | Playback, waveform, seek, loop, volume, associated text |
| Movies | SMK and movie definitions | Video/audio playback, timeline, frame stepping |
| Spatial data | PVO | Decoded tree and references; overlays where mappings are supported |
| Definitions | Supported INI data, SOUP definitions, animation/script descriptors | Searchable tables, groups, instructions, asset references |
| Other data | CHT and remaining files | Known fields or an explicit unsupported reason |

**Verified:** the pinned rgpre `FileType::all()` contains thirteen file types;
its import modules also cover configuration, movie, CD, and script inputs.
Catalog coverage must account for both groups. Archive contents are individually
browsable. Unknown or malformed entries remain visible with contextual errors.
Parsing and recovered format rules stay in preservation; missing rules are
not invented to fill a preview. Parser fixes go upstream before adoption here.

Keep the primary display useful for the content: images and geometry for visual
assets, waveform and playback for audio, decoded tables for structured data.
The final styling pass uses existing egui facilities, consistent spacing,
restrained colors, clear focus/selection, and useful empty/loading/error states.

## Data and loading contract

A catalog entry has a stable identity composed of source path and member
identity, a category, metadata, and either a typed preview description or an
unavailable reason. Preview variants cover image, model, world, audio, movie,
and structured data. Do not identify files only by model name: the game registry
selects preferred sources, whereas the viewer must retain colliding files.

Preparation indexes first and generates selected previews on demand.
An export command must not be a prerequisite for browser use. The browser
fetches source files as needed, rather than every decoded asset at startup.
Switching selections cancels or discards stale results and releases
the previous preview, including audio. Large lists and media need bounded memory.

Reuse `cyrus-config/cache` and compatible game exports. Cache identity includes
source/dependency fingerprints, palette/world context, and schema/exporter
version. Different installs must not silently reuse each other's outputs.
Publish complete exports atomically and detect stale or interrupted exports.
Validate catalog paths and declared sizes at load boundaries.

Both apps use the same loading-bar component and browser download presentation.
Report scan, preparation, and download progress; unknown totals are indeterminate.
The shared UI reports completion without selecting a game or viewer state.

Serve the web apps under one origin with common asset URLs so downloaded assets
actually share Cache Storage. Extend caching beyond GLB/WASM to catalog,
sidecars, images, and media. Keep engine outputs distinct and expose only the
intended asset directory, not local settings. Exports and original assets stay
untracked and are never included in application releases.

## Implementation order

1. Establish the catalog identities, shared ownership, and native/browser
   preview contract. Check the design with one model and its textures, an
   archive member, and a world/palette variant. Build a thin native/WASM viewer
   shell against that contract. Prove the existing Smacker decoder
   can supply a browser video frame and audio with bounded work, user-gesture
   audio activation, and a workable seek path. If direct decoding is unsuitable,
   keep decoding in the browser with a bounded worker path. Browser movies are part of
   the requested scope, not silently reduced to metadata.
2. Extract shared assets/app responsibilities and migrate the game's callers
   and tests. Keep one CLI/cache/loading implementation. Preserve game startup,
   menu, headless export, scene loading, inspector, and Faithful defaults.
3. Implement catalog discovery and exports, including archive members,
   duplicate names, palettes, world variants, and explicit failures. Shared
   media decoding keeps game-specific movie validation and skip/subtitle policy
   in the game wrapper.
4. Complete browsing and each preview family: images/fonts/palettes/models,
   then worlds, audio, movies, and structured data. Reuse supported appearance
   modes where the extraction is small. Apply the egui styling pass.
5. Verify both applications and update crate READMEs, setup instructions,
   generated map, workspace configuration, just recipes, web tooling, and CI.
   Record accepted tooling progress and verification evidence in the owning GitHub issue during coordinated integration.

Commands are `cargo run -p cyrus-viewer -- --install PATH`,
`just viewer-web-build`, and `just run-viewer-web PATH`. Their implementation should reuse the existing build
machinery and worktree-local target directory.

## Acceptance and limits

Use focused public-interface tests for catalog identity/coverage, malformed
inputs, cache invalidation, interrupted publication, explicit install paths,
and preview replacement. Account for every supported family and archive member
in an installed-data run. Exercise representative previews, keyboard/mouse
navigation, audible playback, movie synchronization, and cold/warm/stale cache
behavior in native and actual browser builds. Confirm the install stays unchanged.

Before publishing code, run repository gates, both WASM checks, and a game
regression smoke. Compilation alone does not verify rendering or audio.

**Unknown:** browser movie decoding performance and seeking, the final cost of
rendering-mode reuse, and unsupported model/script interpretations. Resolve
browser media feasibility first; preserve explicit limits for unsupported data.
Asset editing, save editing, script execution, and completing game fidelity are
outside this application's scope.
