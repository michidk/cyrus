# cyrus-assets

Validated portable animation, player, collision, environment, menu, HUD, movie and
sound cache records. Data-only lookup and validation live with their serialized
records. Runtime animation, collision queries, mesh bindings and audio handles
belong to the consuming crates.

The opt-in `native` feature enables read-only install discovery, preload scans,
scene/startup/menu/HUD/movie export, and sound/CD/voice/font preparation through
`rgpre`. It also enables native font conversion in `cyrus-text`. Export paths are
caller-supplied; the install is never written. Existing sidecar versions and
filenames are unchanged. Prepared audio is WAV bytes, never a Bevy audio handle.

Run `cargo test -p cyrus-assets` for portable records, or
`cargo test -p cyrus-assets --features native -- --nocapture` for export contracts.
Installed tests report skips when the install is unavailable. Exported content
stays in the ignored local cache; no game data ships in this crate.

World exports include `.animations` sidecars containing actor animation groups,
RAAN model references, and decoded model frame positions and normals. Missing or
unsupported model references remain explicit in the asset's `unsupported` map.
Original animation payloads and the RASC, RAST, RASB and RAVA script inputs are
retained too. Preservation supplies typed SOUP decoding; cyrus exports version-2 portable
program descriptors with names from the installed SOUP386.DEF. The independent
version-2 `.scripts` sidecar retains actor RASC origins, definitions, fresh-game flag values, locals, strings,
attributes, hooks and explicit unsupported records. Its content identity rejects
changed metadata. Animation metadata retains the same script identity; preparation
rejects animation/script sidecars from different exports. The animation inspection adapter also retains those same
program descriptors in animation metadata.
Embedded ROB models are retained when matching archive payloads agree; conflicting
payloads remain unsupported rather than selecting an archive without evidence.
Native and browser scene loading retain this data. Re-export worlds for schema 2
and the GLB source vertex/face mapping. Older sidecars are rejected with a
re-export error. In `cyrus-render`, explicit model poses update private meshes, normals, and bounds;
removing a pose or changing worlds restores the original mesh.

World export also prepares CYRUS's standalone RAAN frame-source models under
`<world>.player-models/`, with the world's palette, textures and source geometry
mappings. The `.player` sidecar retains their authored model-header radii in map
units for collision candidate selection. Missing standalone player models report
contextual export errors.
Re-export worlds before using explicit player model poses.
