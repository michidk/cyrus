# cyrus-mods

Native-only mod host for the game binary, built on
[wasvy](https://github.com/wasvy-org/wasvy) (WebAssembly components on wasmtime),
with Hanabi for mod particles and Rapier for mod-owned visual physics. It is a
separate crate so wasmtime stays out of the browser build and out of the crates
that define the game; `cyrus-web` never depends on it. The bundled mods, the SDK
and the authoring guide live in [`mods/`](../../mods/README.md).

A mod is a folder `mods/<id>/` beside the game executable with a `mod.toml` and any of
`mod.wasm` (a component built against `wit/cyrus.wit`), `[[shader]]`
replacements, data patches (`[[script-patch]]`, `[[ini-patch]]`,
`[[text-patch]]`) and a `files/` overlay of the install.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `ModsPlugin`: catalog, Mods window, activation (`mods.json`), hot reload, shader slots; `register_source`, `file_overlays` and the patch readers |
| [api](src/api.rs) | Host components behind `wit/cyrus.wit`, exchanged with the engine hooks once per frame |
| [settings](src/settings.rs) | `[[setting]]` declarations, edited in the Mods window, saved to `mod-settings.json` |
| [gameplay](src/gameplay.rs) | Guest-requested services such as saves; the host owns eligibility and completion |
| [ownership](src/ownership.rs) | Loaded guest incarnations shared by asynchronous hooks |
| [particles](src/particles.rs) | Validated, owner-scoped Hanabi bursts |
| [terrain](src/terrain.rs), [geometry](src/geometry.rs) | Bounded terrain snapshot reads and owned mesh uploads |
| [lifecycle](src/lifecycle.rs), [physics](src/physics.rs) | Actor handles, death observations and Rapier-backed visual assemblies |

## Usage

The `cyrus` binary calls `register_source` before the asset plugin, feeds
`file_overlays()` and the script/INI/text patches to the asset readers before
anything scans the install, then adds `ModsPlugin`. The Mods window opens from the
Backtick menu; folders are rescanned every half second, so mods can be added,
removed or edited while the game runs (`files/` and data patches need a restart).

Shader replacements wait for their target assets to load, then remain installed
until the mod catalog changes. A completed load retry does not replace the shader
again on subsequent frames.

## Notes

- Hooks are generic engine services, not features: mod policy (grass placement,
  blood look, ragdoll rigs, autosave timing) stays in the guest. A hook a mod
  stops answering returns to the original behaviour.
- Answers written in `Update` apply in the next frame's `PreUpdate`; damage,
  death and actor readbacks are shared read-only batches.
- Everything a guest submits is validated and budgeted before allocation, and is
  scoped to the mod instance, loaded revision and world generation; disable,
  reload or world replacement releases it. Mod-owned physics never writes gameplay
  state or collision.
- wasvy is pre-1.0: `wit/deps/wasvy-ecs.wit`, `mods/sdk` and the bundled mods
  must stay on the version pinned in `Cargo.toml`.
- Security: wasvy grants mods whole-world access, inherited stdio and network.
  Owner IDs are a lifetime convention, not a sandbox. Only run mods you trust.

## Testing

```sh
cargo test -p cyrus-mods
just mods          # build mods/* for wasm32-wasip2 and install into <target>/debug/mods
```

Ignored tests load real guest components: set `CYRUS_BLOOD_WASM` and run
`wasm_blood_particles`, or `CYRUS_GRASS_WASM` and run `wasm_grass_geometry`
(`cargo test -p cyrus-mods --lib <name> -- --ignored`). The fly-camera exit
checks use `CYRUS_FLYCAM_WASM` and `wasm_flycam`, exercising both values
of `carry_player` and the per-frame `follow` placement through the real guest/host interface. See
[`mods/README.md`](../../mods/README.md#building-and-testing).

The player hook accepts `place` in map units with a heading in 1/2048 turns.
It consumes the last finite request before the next ordinary update, clears
retained movement state through the controller's placement API, and lets
ordinary ground support settle the player. The fly-camera mod uses it when
leaving flight with its `carry_player` setting enabled, and every frame of a
flight with its `follow` setting enabled (both off by default).
