# cyrus-collision

`ActorModelCollision` prepares initial live static-model geometry independently
of mutable actor state. Mixed ground queries retain static/live candidate order
and the supporting actor and face. Callers supply stored model poses in original
map axes. Transient geometry and authoritative live support flags require their
runtime owners and remain unsupported by this registry.

`ModelAdmission` validates prepared PVO references and supplies ordered object IDs
from an original-coordinate camera lookup. It permits shared children, rejects
cycles, and does not choose actor eligibility or rendering culling. Gameplay owns
retained support and the load-time secondary actor table.

`ActorBounds::from_vertices` derives static-actor extents from original model
vertices, with nearest-even conversion and installed horizontal scaling.
Authored MPSZ bounds use the same sphere builder after validation. Gameplay owns
selection of the original loader branch and its model-less single-sphere path.

Terrain support, polygon contacts, model meshes and ordered static-world queries.
Consumes serialized `cyrus-assets::collision` records and Bevy math, without world
lifecycle, gameplay, rendering, an install parser or an audio backend.

`StaticCollision` builds the spatial registry and supplies ordered candidates,
body contacts and support queries. `TerrainGround`, `TerrainMaterial`, probes and
support history retain their existing units, traversal order and contextual errors.
Player admission, movement and wall response remain in `cyrus-game`; original-file
parsing and serialized collision export remain in preservation and `cyrus-assets`.

Native and browser gameplay use this same core. The crate exposes no Bevy plugin
and chooses no simulation clock. A crate extraction does not change cache schemas.

Run `cargo test -p cyrus-collision` for the standalone geometry, support, material
and static-world contracts. Gameplay composition is covered by `cyrus-game` tests.
The isolated consumer in `dev/scripts/check-core-boundaries.sh` checks that runtime
collision does not acquire presentation, native parser or transport dependencies.

**Verified:** the independent polygon-segment and ground-support selection cores
follow [preservation PR #27](https://github.com/michidk/redguard-preservation/pull/27).
They retain distinct terrain/model rank scales and replace earlier equal-ranked
contacts. Callers still supply admission, transforms and traversal order; these
cores do not activate gameplay collision. Nearest-even segment conversion follows
the documented default rounding evidence; a live halfway-boundary comparison
remains unchecked.

World exports also write a `.player` sidecar containing installed movement
settings, named CYRUS attributes and bounds, decoded terrain heights, and the
water mask.
The optional terrain texture grid retains packed image/rotation bytes for material
queries using the [published coordinate rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#terrain-material-lookup).
Old caches require re-export for these queries. Explicit selected-support surface
queries apply the [generated terrain blend mask and water overrides](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#terrain-material-blend-coverage).
They do not assign actor surface state or the shared movement veto. Terrain water
height and buoyancy remain unsupported. Schema 2 also retains original static
placement order, model polygons, ROB collision flags and ordered subobject face
lists. Per-face texture set/image
identity is retained for surface lookup. The cache includes the installed
SURFACE.INI assignments, and textured contacts resolve their surface code.
Version-4/5 solid-color contacts resolve to surface zero.
Older caches without face textures or assignments cannot supply surface-dependent
behavior. Missing model-version metadata requires re-export.
Models resolve from the WORLD.INI-selected world archive in authored segment order. Missing models remain
explicitly unsupported. Authored version-4 subobjects are retained in the cache
but discarded during runtime preparation, matching the original loader.
Native, headless, and browser entry load the same validated asset.
Missing or unsupported actor definitions retain a contextual unavailable reason.
Loading also prepares static collision meshes and their spatial registry.
Explicit queries preserve view-centered cell order, separate actor-style
candidates, transform original polygons, and retain segment contact order.
Explicit static body probes test actor spheres against authored model faces and
return ordered contacts for response. Coincident subgroup centers and saturated
body-contact buffers remain unsupported.
Archive selection follows [preservation PR #38](https://github.com/michidk/redguard-preservation/pull/38);
segment admission and traversal follow [PR #41](https://github.com/michidk/redguard-preservation/pull/41).
Sphere model admission and forward slope/support selection follow the
[published player facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md).
The caller supplies the retained surface veto after support-state updates;
geometric support alone does not authorize forward movement.
`primary_support` selects ordinary actor support without player fallback or
shared history updates; a primary miss stays absent.
When primary support is absent, the selected-player fallback uses the caller's
support-history state and retains the actor's runtime Y at the first accepted
non-side contact. This follows the [published fallback rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#selected-player-fallback-support).
`runtime_fallback_support` applies the same secondary probe to retained static
and live face candidates. Its result preserves the supporting actor or model and
face, and does not inspect later candidates after an accepted contact. Runtime
history and surface assignments remain host-owned.
Explicit selected-player queries also commit shared support history. Misses retain
metadata, so they can reset the fallback timer; temporary forward probes retain
those changes when position is restored. The caller supplies initial state and
nominal ticks, following the [support-state rules](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#support-query-misses).
Ground support reads only face-model candidates. Actor-style obstacle response
remains unsupported.
Startup support uses an actor-centered window and radius 1000, with a vertical
half-length of 500. Its distinct terrain/model ranking retains earlier equal
contacts, following [preservation PR50](https://github.com/michidk/redguard-preservation/pull/50).
The game uses this static query for inspection placement. Dynamic/transient support
and automatic locomotion remain unsupported. Re-export cached worlds after updating.

Actor collision shapes share one map-coordinate builder for upright, X-dominant and Z-dominant authored bounds. Ordinary obstacle spheres remain separate from the additional stored sphere. The selected player consumes the same builder. Construction follows [preservation’s actor shape rules](https://github.com/michidk/redguard-preservation/blob/e33cfa2/docs/engine/player.md#horizontal-actor-collision-spheres). This prepares geometry; ordinary NPC motion/support/contact stage integration remains incomplete.

Runtime candidate preparation visits the static spatial list, ordinary live actors and transient models in original order. Separate face and actor-style lists each retain their 64-entry limit, model-instance identity, owners and collision metadata. Existing static queries use the same selection core. The host supplies current loaded-model bounds and authoritative exclusion state. This prepares candidate records; live model geometry, ordinary NPC support/response and transient runtime owners remain incomplete. Rules follow [preservation’s candidate construction](https://github.com/michidk/redguard-preservation/blob/192fd01/docs/engine/player.md#live-and-transient-collision-candidates).

Mixed primary body queries retain static/live owners and nearest-even integer
world normals. Model-local queries retain the first thirty-two successful faces;
the primary collector returns after sixteen model contacts before later models
or terrain. Invalid body shapes fail even with no admitted candidates. These
queries follow preservation's [body-query contracts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md#primary-body-contact-collection).
Response and transient geometry remain owned by the caller.

Residual body collection scans all admitted static/live face models without the
primary contact cap or terrain addition. It retains model-local limits and
caller-supplied working spheres. Ordinary gameplay owns state-dependent radius
adjustment, angle snapshots, separation and result flags.

Actor-style overlap queries consume host-transformed selected spheres, visit the
first thirty-two in shape order, and return horizontal separation from the first
strict three-dimensional overlap. Static placement and actor-pair radius truncation
remain distinct, following [preservation's overlap rules](https://github.com/michidk/redguard-preservation/blob/6e0102f/docs/engine/player.md#actor-sphere-overlap).
Shared original distance arithmetic also supplies SOUP spatial queries. Gameplay
still owns selected-shape preparation, self exclusion and weighted response.

Actor-style static sphere lookup validates retained placement/model identities
and collision flags before returning the stored placement origin and model
radius. It does not re-admit candidates or substitute face-model geometry.

**Verified:** mixed segment queries accept explicit map-unit endpoints and preserve retained
static/live face order and duplicates. Candidate-type exclusion precedes live
pose resolution. The world integer adapter rounds points and scaled normals
nearest-even without normalizing or changing rank, following preservation's
[ordinary segment rules](https://github.com/michidk/redguard-preservation/blob/729dc5b/docs/engine/player.md#ordinary-forward-segment-response).

`runtime_forward_support` composes mixed-candidate body clearance, optional slope
rejection, primary support and fallback. `Blocked` skips support-state effects;
`Queried(None)` is a completed miss whose retained history still needs updating.
Body rejection stops at the first blocking contact before later candidate reads.
