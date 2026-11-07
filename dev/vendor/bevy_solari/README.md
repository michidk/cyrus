# Solari BLAS compaction queue

Vendored from the published bevy_solari 0.20.1 crate, under MIT OR Apache-2.0.
Library sources, manifest and licenses are retained.

`prepare_raytracing_blas` queues each new BLAS for compaction but leaves the
entries of the BLAS it replaces in the queue. When a mesh is modified before its
compaction finishes, the replacement is prepared twice and wgpu reports
`Blas::prepare_compact_async: Compaction is already being prepared` every frame.
The queue now drops the entries of every removed or modified mesh before the
replacements are queued, so each mesh has at most one entry.

Remove this copy when an upstream release carries the same fix.
