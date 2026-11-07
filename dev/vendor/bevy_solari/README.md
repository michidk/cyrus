# Vendored bevy_solari

Vendored from the published bevy_solari 0.20.1 crate, under MIT OR Apache-2.0.
Library sources, manifest and licenses are retained. Two changes, both to remove when an upstream
release carries an equivalent.

## BLAS compaction queue

`prepare_raytracing_blas` queues each new BLAS for compaction but leaves the
entries of the BLAS it replaces in the queue. When a mesh is modified before its
compaction finishes, the replacement is prepared twice and wgpu reports
`Blas::prepare_compact_async: Compaction is already being prepared` every frame.
The queue now drops the entries of every removed or modified mesh before the
replacements are queued, so each mesh has at most one entry.

## Denoiser

Upstream leaves its output noisy and expects DLSS Ray Reconstruction, which needs NVIDIA's SDK and
hardware. `SolariLighting::denoise` adds a spatio-temporal denoiser to the end of the lighting
pass (`realtime/denoise.wesl`, wired in `realtime/node.rs` and `realtime/prepare.rs`) that runs
on any GPU Solari runs on. It divides the albedo out of the lit image, accumulates the light over
time with a reprojection validated against the previous frame's depth and normals, filters it with
three edge-stopping a-trous iterations whose luminance tolerance narrows as the history grows, and
multiplies the albedo back in. It needs `restir` for the previous-frame buffers and treats all
light as diffuse, so it suits matte surfaces.
