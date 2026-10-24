# cyrus-assets

Script cache schema 29 retains the placement texture word of type-3 objects (the flat sprite records map loading creates), the installed rope jump-off speed and attach window, the installed falling-death limit in nominal ticks, the
installed lineup distance in Q8 units, and the frame count of every installed texture bank
image. It also lists the world's shell scripts (the debug shell unless SYSTEM.INI `final_version`
is set, `COMMON`, and the script named after the map file) in creation order. Re-export older
script caches; omitted limits remain unavailable.

Player records retain optional effective installed `swim_depth` in map units for
model-height displacement. Preparation replaces configured zero with 40; older
records retain unknown depth. Catalog version 26 invalidates older generated caches.

Validated portable animation, player, collision, environment, menu, HUD, movie and
sound cache records. Data-only lookup and validation live with their serialized
records. Runtime animation, collision queries, mesh bindings and audio handles
belong to the consuming crates.

Collision caches retain shared authored geometry separately from static placement
flags. Initial type-one live static models retain their MPOB bindings and model
header radii and initial ROB collision metadata, including known zero for ordinary
actors without a static resource. Runtime hosts retain that metadata and supply
current poses. Collision scenes retain the effective installed sphere-object scale
for initial static bounds. Missing live geometry or metadata remains unsupported;
catalog version 24 requires preparation of older caches. Animation models also
retain the original header radius.

The opt-in `native` feature enables read-only install discovery, preload scans,
native filesystem and disk-cache adapters. The `parsers` feature enables shared
scene/startup/menu/HUD/movie and sound/CD/voice/font preparation through `rgpre`
on native and browser targets. `game::prepare` returns cache-relative bytes from
a `ReadAssets` source; the browser worker requires no server-side exporter. Export paths are
caller-supplied; the install is never written. Sidecar filenames are stable; schema checks reject incompatible cache records.
Native and browser exports use the same math backend for placement transforms through
preservation's [portable export](https://github.com/michidk/redguard-preservation/commit/8c6bdb15c783fca72a6399aa226f25f596585407).
Re-export manually prepared worlds and models after updating. Session documents
retain exact source identities and reject incompatible exports.
Environment preparation also retains installed effect pool settings and the four
selected-death texture images, with complete palette-decoded frames, source dimensions,
texture scale, timing, map `FLAT` drawing attributes and signed per-world effect
brightness. Older caches report absent brightness rather than an invented default.
Re-export older environments before consuming effect inputs; absent metadata
remains unavailable. This prepares
data only; effect spawning and presentation are not enabled by the cache.
Environment caches require the installed 256-size and 128-size texture reduction
factors. Re-export older environments and model GLBs for original pixel-coordinate
metadata before using fixed model sampling.
Environment caches retain exact static placements and ordered signed map-light
inputs for world-entry prelighting, plus authored kind-6 object lights separately
for runtime shading. Object loading follows SYSTEM.INI; active object-light
scripts remain explicitly unsupported. Model GLBs retain the original bound and
palette RGB through preservation metadata; old lighting caches require re-export.
Positioned world nodes retain selected ROB render attributes separately from shared
mesh geometry; standalone and generated models retain null ROB provenance. Re-export
worlds for version-1 `rgpre_render` metadata. Native effects and exported audio are WAV bytes, never a Bevy audio handle.
Placed MPSF nodes retain preservation's `rgpre_flat` source-image dimensions, scale
and drawing attributes. Re-export worlds before using the renderer's flat geometry.
Standalone player exports retain no flat metadata.
Sound banks require validated SURFACE.INI rows from preservation PR #65. Shared
preparation exports them for the browser, and native audio reads the same parser.
Older sound-bank metadata must be re-exported; malformed row sizes are rejected.
Native movie preparation retains a read-only file or CD-image location and reads
only metadata. Its bounded reader closes the CD image when dropped. Browser movie
preparation loads metadata only; `cyrus_media::MovieStream` consumes compressed
chunks and discards decoded packets. The vendored `smk` codec adds sequential
packet input without replacing the upstream decoder.

Run `cargo test -p cyrus-assets` for portable records, or
`cargo test -p cyrus-assets --features native -- --nocapture` for export contracts.
Installed tests report skips when the install is unavailable. Exported content
stays in the ignored local cache; no game data ships in this crate.

The installed `every_world_exports_valid_animation_geometry` test exports every
WORLD.INI entry, including shared-map palette variants, and validates all exported
animation models and frames, including each authored ShowFrame reference through
its actor's RAAN table or each type-one placement's model frame table. Definitions
without a supported instance frame source are reported separately. It fails on
unsupported model references. This checks
asset coverage, not automatic animation playback or script progression.

World exports include `.animations` sidecars containing actor animation groups,
RAAN model references, and decoded model frame positions and normals. Missing or
unsupported model references remain explicit in the asset's `unsupported` map.
Original animation payloads and the RASC, RAST, RASB and RAVA script inputs are
retained too. Preservation supplies typed SOUP decoding; cyrus exports version-2 portable
program descriptors with names from the installed SOUP386.DEF. The independent
version-15 `.scripts` sidecar retains the validated SYSTEM.INI clock and ordinary-update switches, the installed actor-pool capacity, actor RASC
origins, definitions, fresh-game flag values, locals, strings,
initialized attributes, typed native hooks and explicit unsupported records. Placement records retain
the original model name and complete model-mode byte for actor initialization.
Menu cache schema 15 preserves authored Save/Continue flags for caller-specific
runtime restrictions. Re-export older menu caches.

The menu cache also carries the clock settings, using the same typed conversion
and validation as world scripts. Its source stamp invalidates older menu exports.
Preparation includes installed pages 0 through 7, including Save and Load layout.
Older caches missing those pages require preparation again. Page data alone does
not enable save-slot handlers.
Actor-load defaults come from preservation [PR #59](https://github.com/michidk/redguard-preservation/pull/59)
and installed SYSTEM.INI; model-path message-group assignment remains separate. Its content identity rejects
changed metadata. Animation metadata retains the same script identity; preparation
rejects animation/script sidecars from different exports. The animation inspection adapter also retains those same
program descriptors in animation metadata.
Embedded ROB models are retained when matching archive payloads agree; conflicting
payloads remain unsupported rather than selecting an archive without evidence.
Native and browser scene loading retain this data. Re-export worlds for schema 2
and the GLB source vertex/face mapping. Older sidecars are rejected with a
re-export error. Each model also retains authored corner normals, resolved by
preservation's shading selection and shared by every frame; caches without them are
rejected. In `cyrus-render`, explicit model poses update private meshes, normals, and bounds;
removing a pose or changing worlds restores the original mesh.

World export also prepares every actor's RAAN frame-source models under
`<world>.player-models/`, with the world's palette, textures and source geometry
mappings. An actor with a RAEX texture set gets its own copy, `<MODEL>-<set>.glb`, with
preservation's [actor texture sets](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/RGM.md#actor-texture-sets)
applied; actors without one share `<MODEL>.glb`. The existing cache directory name is shared by player and ordinary actor
models. The `.player` sidecar retains only CYRUS's authored model-header radii in map
units for collision candidate selection. Frame tables and replacement geometry use
the same placed, standalone or embedded ROB source selection. Missing or conflicting
sources report contextual export errors. Re-export worlds before using newly supported frame-source models.
Ordinary actor model replacement and session staging are assembled by the runtime
session replacement path; this export layer supplies the validated frame-source
geometry and metadata consumed by that path.

**Verified:** the pinned preservation [texture export](https://github.com/michidk/redguard-preservation/commit/d0eec6c98e6917f942e643204fc155f53b34b649)
retains every authored TEXBSI frame in world and model GLBs, with shared image
identity, frame order and the installed delay in material extras. Frame zero
remains the base-color texture. The viewer catalog version invalidates older
model previews; re-export manually prepared worlds. Runtime texture playback
and its render-admission rules remain unsupported.

**Verified:** the pinned [polygon export](https://github.com/michidk/redguard-preservation/commit/a4b007242d7a72eea1a54a42d5bd7afe57d0d459)
retains authored polygon corners, source face identity, face normals and model
bounds independently of raster triangulation and lighting normals. Installed
START, ISLAND and BELLTOWR checks compare these records with parsed models.
Re-export worlds and model previews for this metadata; it does not enable
runtime texture animation.

**Verified:** preservation's [terrain texture export](https://github.com/michidk/redguard-preservation/commit/9ac35c15c66b9a5671320e42951e0a53e05ae39e)
keeps all terrain frames opaque and preserves palette-index-zero RGB. Model
textures retain their chroma transparency. Re-export existing worlds for this
correction; the viewer catalog version invalidates older previews.

## Asset viewer

The `parsers` feature enables runtime parsing on native and browser targets; `native` includes it.

Shared, renderer-independent asset handling for the game and asset viewer.
Preservation (`rgpre`) owns parsing and recovered engine rules. This crate owns
install validation, catalog identities, portable preview descriptions and generated
files. `cyrus-media` owns PCM conversion and the portable movie decoder wrappers.

Native game and viewer share `GameInstall::locate`, which resolves selected
library/package folders and install descendants through `GameInstall::resolve`.
Internal callers use `GameInstall::open` to validate an exact root, then
`native::Library`. Discovery preserves files with duplicate names and lists archives without reading them.
`expand` reads a selected archive index and gives members separate IDs; `prepare`
decodes one selected member from its byte range at runtime. Model previews accept
an optional palette from their world references; absence selects the first known
palette, while an explicitly empty selection uses embedded colors or grayscale
indices. Model frame poses come from preservation's shared conversion API. Palette choices have separate cache identities.
Original filenames containing `#` are accepted by native and browser catalogs.
HTTP readers encode source path components; generated cache paths remain free of URL syntax.
Unknown files and decoder failures remain inspectable catalog entries.
The library retains the most recently used source or archive index, so members
reuse indexing without retaining all payloads in memory.

The native exporter writes only to the supplied cache directory. A generation
hash includes the original-file manifest and schema version. Changed export semantics must bump `CATALOG_VERSION`.
Complete files are published by rename. Invalid preview manifests and missing
media are regenerated. Old generations remain recoverable and can be removed
manually when no client needs them.

Both targets use `runtime::Library` and its read-only `ReadAssets` interface.
Native adapters read files; the browser worker reads selected local File handles
or original bytes over HTTP.
Generated media is returned as bytes. Only native callers write a disk cache.
Browser presentation registers generated files with Bevy in memory.

Run viewer contracts with `cargo test -p cyrus-assets --features native -- --nocapture`. Installed-data
checks use `REDGUARD_DIR`, or the repository's test install link. They explicitly
skip when neither is available. No game data is included in tests.

The pinned preservation [runtime loading fixes](https://github.com/michidk/redguard-preservation/pull/51)
keep terrain arrays off the WASM stack, support archive index reads, and expose
texture bank resolution so models load only their referenced banks.

The pinned [model frame snapshot fix](https://github.com/michidk/redguard-preservation/pull/51)
exposes compressed and full-precision animation frames through the shared parser API.
The static SOUP census visits every WORLD.INI entry separately, including shared
maps with different palettes, and every decoded actor definition, including
unplaced standalone actors. It counts instructions, nested native calls, operand
and reference forms, native hooks, and unsupported records. Function declarations remain
separate from observed call sites. Counts describe authored occurrences, not
reachability, runtime support, exercised operations, or original-game parity.
World preparation failures remain in the report and cause a failing exit status.

```sh
mkdir -p magic/artifacts/soup-inventory
cargo run -p cyrus-assets --features native --example soup_inventory > magic/artifacts/soup-inventory/census.json
```

Keep this generated installed-content report under `magic/`. Handler ownership,
missing services, and scenario evidence belong to the completion investigation;
the asset census does not infer them from function names.

Placed-object frame lookup uses the instance metadata retained by preservation,
following [the frame-source rules](https://github.com/michidk/redguard-preservation/blob/6488b6e/docs/engine/animation.md#placed-object-frame-sources).
This lookup does not establish automatic actor or animation eligibility.

`cheats` retains all CHT slots and names in a portable record. Its `parsers`
adapter delegates binary decoding and encoding to preservation's
[CHT writer](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/CHT.md).
The game core depends only on the record; host storage owns file access.

`scene::export_scene_observing` runs the normal exporter with an optional audit
observer. It retains skipped placement counts and texture fallback reasons through
preservation's [merged reporting extension](https://github.com/michidk/redguard-preservation/commit/9fde2b567605b86d458ec8863558aa2b994f0cb9).
**Verified:** upstream main includes this isolated reporting contribution. The
current dependency pin includes the earlier implementation; the linked commit
identifies the merged contribution, not the dependency pin's ancestry.
The observer changes neither asset selection nor fallback behavior. The native
`cyrus --audit-worlds` command combines these records with preload and runtime
preparation diagnostics. Generated installed-content reports belong under `magic/`.

`CheatSeed` carries read-only installed CHT bytes and SYSTEM.INI startup defaults
for the browser host. `console` prepares the installed paper, configured FNT and
per-world palette inks for native and browser presentation.
World preparation reads only the supported PlayerDistance seed from CHT. Other
saved slots do not block loading; the runtime retains their values and reports
unsupported effects. SYSTEM.INI supplies startup task and animation settings.
Generated filesystem paths use portable slash-separated cache keys on every platform,
including Windows. Absolute paths and traversal remain rejected.

Native preload failures retain install-relative filenames, including directories and extensions, so audit diagnostics identify the source file without depending on the local install location.

Catalog version 19 invalidates older browser export caches. Script schema 12 retains authored RALC offsets in Q8 units and validated installed gameplay dialogue settings, including effective allocation minima. These settings participate in script content identity; older caches require preparation again.
Each actor also retains its consecutive combat-taunt list from RAEX and
`attr_num_taunts`. Preparation reads COMBAT.INI through preservation, retains
four-byte labels, expands missing sections to empty records, and resolves
nonzero combat animation offsets. Taunts participate in script content identity.
These records do not enable combat or load voice assets by themselves.
Preparation uses preservation's validated per-header location parser; original
home positions are assigned per instance by the game. Location offsets participate
in script content identity. The parser increment is documented in
[preservation's RALC documentation](https://github.com/michidk/redguard-preservation/blob/main/docs/formats/RGM.md#ralc-location-data).

With `parsers`, `sound::dialogue` reads selected installed RTX ranges. Portable
`sound::DialogueCache<T>` retains twenty owned payloads without parser
dependencies. A cache-miss reader supplies the payload; `DialoguePayload` exposes
voice presence and text release while retaining audio ownership. Native readers
use `RtxEntry`; runtime hosts can retain prepared records and audio handles.
Maintenance requires synchronous
sample status and stop feedback. Playing payloads do not age. Lookup reuses an
authored label or replaces the first entry not marked used; exhausting the used
entries returns an error. `replace_taunt` drops the actor-held cache reference,
releases the old payload only when it remembers a sample, and stops the requested
label's cached sample before retaining its new reference. This follows
[preservation's combat cache lifecycle](https://github.com/michidk/redguard-preservation/commit/5393814ce34c2d43a12a793c17df4eec8fc4a636).
The caller prepares dependencies before this synchronous operation. These loading APIs do not enable automatic dialogue
tasks or presentation by themselves. `game::prepare` also accepts
`dialogue/<eight lowercase hex digits>.dialogue` and `.wav`, prepares only that
authored four-byte key, and retains original text and voice lifetimes plus PCM
metadata. Native `sound::dialogue::export::export` writes the same output.
The current original-text lifetime adapter supports ASCII without embedded NUL;
other text encodings remain explicitly unsupported.

SOUP program types and operand/definition validation belong to
`cyrus-script::program`. This crate prepares those records from preservation,
owns world/actor cache metadata and loads the cache through Bevy. Serialization
and cache identities are independent of the Rust module that owns each record.

HUD preparation reads SYSTEM.INI's configured item database through preservation's
typed ITEM.INI parser and exports all item
definitions and unhidden artwork, including items not yet owned. Initial counts
and ordered starting acquisitions remain separate installed inputs; gameplay
applies startup and subsequent acquisition through the same owner. Ordered
prerequisites, drop/add dependencies and removal flags travel with each definition.
HUD metadata retains whether each installed item has type one for shared weapon
validation and selection fallback. The inventory screen's artwork travels with it:
the `bitmap_selected_file` frames (`hud/selected-NN.png`), `INVBORDR.GXA`
(`hud/invbordr-00.png`) and `INVARROW.GXA` (`hud/invarrow-NN.png`), plus each item's
`name` RTX label and the three frame sizes. Re-export older HUD caches for the required
inventory metadata.

`logbook/reader.logbook` contains the installed paper, small fonts, bindings,
scroll geometry and the parsed `CREDITS.TXT` records (absent when the install has none);
re-export older caches. The shared reader group prepares it on demand in browser workers;
native HUD preparation exports the same validated record.

`overlay/<name>-<frame>.png` holds one frame of an installed `system` GXA image, decoded with
the palette the file carries. The `overlay` group prepares it on demand for the images a
`ViewGXA` script task names, in native and browser workers; `overlay::cache_path` maps the
script's file name and frame to the path.

Script caches retain contiguous combat attack definitions, ordered combo motions,
resolved animation groups and runtime numeric fields. Motion preparation applies
preservation's omitted-value conversion; raw INI parsing retains omissions.
Runtime queries still reject missing fields in incomplete captures.

Script caches retain the optional installed fighting-query threshold in Q8 units
and the unscaled `COMBAT.INI` neighbor-spacing threshold. Both participate in
content identity; older caches require re-export.

Script instances retain authored signed light intensity, radius and RGB before
runtime object-kind admission. Older script caches require re-export.

Environment exports retain the complete terrain height grid in engine units,
including the signed per-world height offset supplied by preservation's
[terrain parser](https://github.com/michidk/redguard-preservation/pull/99).
Terrain playback does not recover those values from GLB positions. Re-export
older environments and recapture session documents before loading them with
this height-aware terrain owner; incomplete terrain sources reject explicitly.

Script cache schema 17 retains the contiguous installed defense table separately
from the attack table, including converted animation groups and authored omissions.
Native and browser preparation share this owner. Older script caches require re-export.

Script cache schema 18 also retains the signed, unscaled installed `node_timer`
in content identity. Native and browser preparation share the export; older
script caches require re-export before perception maintenance can use it.

Script cache schema 19 retains the effective dummy 3D manager setting used by
animation-vertex queries. Native and browser preparation derive it from installed
manager and static-angle conversion settings. Older script caches require re-export.

Script cache schema 20 retains installed combat impact cues in content identity.
Re-export older script caches before using combat contact responses.
