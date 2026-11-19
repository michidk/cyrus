# cyrus-collision

Terrain support, polygon contacts, actor shapes and ordered static-world queries
in original map coordinates. It consumes serialized `cyrus-assets` collision
records and Bevy math only: no world lifecycle, gameplay, rendering, install
parser or audio backend. It exposes no plugin and chooses no clock. Native and
browser gameplay use the same core; `cyrus-script` owns player admission, movement
and wall response, and preservation plus `cyrus-assets` own file parsing and the
cache export.

## Modules

| Module | Responsibility |
| --- | --- |
| [collision_world](src/collision_world.rs) | `StaticCollision`: spatial registry, ordered candidates, body probes, `primary_support`, `runtime_forward_support` |
| [ground](src/ground.rs) | `TerrainGround` height/support and `TerrainMaterial` surface lookup |
| [geometry](src/geometry.rs) | Polygon-segment contacts and ground-support selection |
| [collision_mesh](src/collision_mesh.rs) | Authored model queries: segment and sphere admission |
| [candidates](src/candidates.rs) | Ordered static, live-actor and transient candidate lists |
| [actor_models](src/actor_models.rs) | `ActorModelCollision`: prepared live static-model geometry |
| [actor_shape](src/actor_shape.rs) | `ActorBounds` and authored actor collision spheres |
| [actor_overlap](src/actor_overlap.rs) | Actor-style sphere overlap and separation |
| [model_admission](src/model_admission.rs) | `ModelAdmission`: PVO-based model selection, independent of render culling |
| [distance](src/distance.rs) | Original integer distance arithmetic, also used by SOUP spatial queries |

## Notes

- Keep original ordering, limits and rounding: candidate order, equal-rank
  tie-breaking, per-model contact caps and nearest-even conversion are part of
  the contract. The rules are in preservation's
  [player facts](https://github.com/michidk/redguard-preservation/blob/main/docs/engine/player.md).
- Callers supply admission, transforms, support history and nominal ticks;
  geometric support alone does not authorize movement (the surface veto is the
  caller's).
- Queries read the world cache (`.player` sidecar, static placements, model
  polygons, face textures, SURFACE.INI assignments). Older caches lack some of
  these and must be re-exported; missing data stays an explicit unsupported result.
- Still unsupported: terrain water height and buoyancy, transient geometry owners,
  actor-style obstacle response and full ordinary NPC collision integration.

## Testing

```sh
cargo test -p cyrus-collision
dev/scripts/check-core-boundaries.sh   # keeps presentation, parser and transport deps out
```

Tests in `tests/` cover terrain, materials, model meshes, candidates, support and
overlap. Gameplay composition is tested in `cyrus-script`.

Static view windows clamp to the map edges for out-of-map cameras, using the
same preserved regional window rule as ordinary actor traversal.
