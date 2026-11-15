# Vendored bevy_solari

Vendored from the published `bevy_solari` 0.20.1 crate, under MIT OR
Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).
Library sources and manifest are retained. The workspace `[patch.crates-io]`
points Bevy at this copy; `cyrus-render` uses it only with the `raytracing`
feature. Remove each change when an upstream release carries an equivalent.

## Changes

- **BLAS preparation** (`src/scene/blas.rs`): upstream re-queues a replaced
  BLAS for compaction, so wgpu reports `Compaction is already being prepared`
  every frame, and it rebuilds every animated mesh's BLAS every frame. This
  copy drops the queue entries of removed or modified meshes, compacts only new
  meshes, keeps tracing a same-size mesh's previous BLAS while its rebuild waits,
  and limits in-place rebuilds per frame.
- **Denoiser** (`src/realtime/denoise.wesl`, wired in `node.rs` and
  `prepare.rs`): upstream expects DLSS Ray Reconstruction, which needs NVIDIA
  hardware. `SolariLighting::denoise` adds a spatio-temporal, edge-stopping
  denoiser that runs on any GPU Solari runs on. It needs `restir` for the
  previous-frame buffers and treats all light as diffuse.
- **Exported lighting system**: `realtime::solari_lighting` is public so the
  traced fog pass in `cyrus-render` can order itself after it.

## Testing

Build with the feature, for example `cargo run -p cyrus --features raytracing`,
and switch ray tracing on under Modern lighting (`--ray-tracing on` or the
control port's `ray_tracing`). It needs a GPU with ray queries.
