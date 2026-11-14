use alloc::collections::VecDeque;
use bevy_asset::AssetId;
use bevy_ecs::{
    resource::Resource,
    system::{Res, ResMut},
};
use bevy_mesh::{Indices, Mesh};
use bevy_platform::collections::{HashMap, HashSet};
use bevy_render::{
    diagnostic::{DiagnosticsRecorder, RecordDiagnostics},
    mesh::{
        allocator::{MeshAllocator, MeshBufferSlice},
        RenderMesh,
    },
    render_asset::ExtractedAssets,
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
};

/// After compacting this many vertices worth of meshes per frame, no further BLAS will be compacted.
/// Lower this number to distribute the work across more frames.
const MAX_COMPACTION_VERTICES_PER_FRAME: u32 = 400_000;

/// Under the `wgpu_hal` build path, we need to manage BLAS lifetimes ourselves.
/// Since solari keeps both current and previous frame TLAS's around, only after
/// two TLAS builds since we marked it for deletion is it safe to delete a BLAS.
const TLAS_BUILDS_BEFORE_DELETION_ALLOWED: usize = 2;

/// Meshes edited in place whose BLAS is rebuilt per frame, at most; the rest keep tracing their
/// previous BLAS until their turn. Animated meshes are edited every frame, and a BLAS per mesh per
/// frame cost more than the rest of the scene. An instance moves through its transform, which is
/// written every frame regardless, so a deferred rebuild only delays a change of shape.
const MAX_BLAS_REBUILDS_PER_FRAME: usize = 32;

#[derive(Resource, Default)]
pub struct BlasManager {
    blas: HashMap<AssetId<Mesh>, Blas>,
    /// Vertex and index count each BLAS was built for; an edit that keeps both can be deferred.
    sizes: HashMap<AssetId<Mesh>, (u32, u32)>,
    /// Meshes edited in place whose rebuild waits, oldest first.
    deferred: VecDeque<AssetId<Mesh>>,
    deferred_set: HashSet<AssetId<Mesh>>,
    compaction_queue: VecDeque<(AssetId<Mesh>, u32, bool)>,
    changed: Vec<AssetId<Mesh>>,
    /// BLAS that are pending deletion, one batch per TLAS build. The back batch collects
    /// retirements since the last build, and every batch ahead of it has one more build to wait
    /// out.
    pending_deletions: VecDeque<Vec<Blas>>,
}

impl BlasManager {
    pub fn get(&self, mesh: &AssetId<Mesh>) -> Option<&Blas> {
        self.blas.get(mesh)
    }

    pub fn device_address(&self, mesh: &AssetId<Mesh>) -> Option<u64> {
        self.blas.get(mesh)?.handle()
    }

    pub fn changed_meshes(&self) -> &[AssetId<Mesh>] {
        &self.changed
    }

    pub fn note_tlas_build(&mut self) {
        if !self.pending_deletions.is_empty() {
            self.pending_deletions.push_back(Vec::new());
        }
    }

    fn insert(&mut self, mesh: AssetId<Mesh>, blas: Blas) {
        if let Some(old) = self.blas.insert(mesh, blas) {
            self.retire(old);
        }

        self.changed.push(mesh);
    }

    fn remove(&mut self, mesh: AssetId<Mesh>) {
        self.changed.push(mesh);
        self.sizes.remove(&mesh);
        if self.deferred_set.remove(&mesh) {
            self.deferred.retain(|deferred| *deferred != mesh);
        }

        if let Some(removed) = self.blas.remove(&mesh) {
            self.retire(removed);
        }
    }

    fn retire(&mut self, blas: Blas) {
        match self.pending_deletions.back_mut() {
            Some(batch) => batch.push(blas),
            None => self.pending_deletions.push_back(vec![blas]),
        }
    }
}

pub fn prepare_raytracing_blas(
    mut blas_manager: ResMut<BlasManager>,
    extracted_meshes: Res<ExtractedAssets<RenderMesh>>,
    mesh_allocator: Res<MeshAllocator>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut diagnostics: Option<ResMut<DiagnosticsRecorder>>,
) {
    blas_manager.changed.clear();

    let extracted: HashMap<_, _> = extracted_meshes
        .extracted
        .iter()
        .map(|(asset_id, mesh)| (*asset_id, is_mesh_raytracing_compatible(mesh)))
        .collect();
    let slices = |asset_id: &AssetId<Mesh>| {
        Some((
            mesh_allocator.mesh_vertex_slice(asset_id)?,
            mesh_allocator.mesh_index_slice(asset_id)?,
        ))
    };

    // Delete BLAS for deleted meshes, and for modified ones that can no longer be traced or whose
    // vertex or index count changed. A modified mesh of the same size keeps tracing its previous
    // BLAS until its rebuild, but is marked changed so its instances pick up the new buffer
    // ranges.
    let mut stale = extracted_meshes
        .removed
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut rebuild_now = Vec::new();
    for asset_id in &extracted_meshes.modified {
        let same_size = blas_manager.sizes.get(asset_id).is_some_and(|&size| {
            slices(asset_id).is_some_and(|(vertex, index)| {
                size == (vertex.range.len() as u32, index.range.len() as u32)
            })
        });
        if same_size && extracted.get(asset_id).copied().unwrap_or(false) {
            blas_manager.changed.push(*asset_id);
            if blas_manager.deferred_set.insert(*asset_id) {
                blas_manager.deferred.push_back(*asset_id);
            }
        } else {
            stale.insert(*asset_id);
        }
    }
    for asset_id in &stale {
        blas_manager.remove(*asset_id);
    }
    // New meshes, and modified ones whose BLAS was dropped, are built now.
    for (asset_id, compatible) in &extracted {
        if *compatible && !blas_manager.blas.contains_key(asset_id) {
            rebuild_now.push(*asset_id);
        }
    }
    let new = rebuild_now.len();
    while rebuild_now.len() - new < MAX_BLAS_REBUILDS_PER_FRAME {
        let Some(asset_id) = blas_manager.deferred.pop_front() else {
            break;
        };
        blas_manager.deferred_set.remove(&asset_id);
        rebuild_now.push(asset_id);
    }
    // A queued entry outlives the BLAS it was made for, and would prepare its replacement a
    // second time. Keep one entry per mesh.
    let rebuilt: HashSet<_> = rebuild_now.iter().copied().chain(stale).collect();
    if !rebuilt.is_empty() {
        blas_manager
            .compaction_queue
            .retain(|(asset_id, _, _)| !rebuilt.contains(asset_id));
    }

    if rebuild_now.is_empty() {
        return;
    }

    // Create a BLAS for each mesh built this frame. Only new meshes are compacted: a mesh edited
    // in place is rebuilt again soon, and compacting it would only build a second BLAS.
    let blas_resources = rebuild_now
        .iter()
        .enumerate()
        .filter_map(|(order, asset_id)| {
            let (vertex_slice, index_slice) = slices(asset_id)?;

            let (blas, blas_size) =
                allocate_blas(&vertex_slice, &index_slice, asset_id, &render_device);

            blas_manager.insert(*asset_id, blas);
            blas_manager.sizes.insert(
                *asset_id,
                (blas_size.vertex_count, blas_size.index_count.unwrap_or(0)),
            );
            if order < new && !extracted_meshes.modified.contains(asset_id) {
                blas_manager
                    .compaction_queue
                    .push_back((*asset_id, blas_size.vertex_count, false));
            }

            Some((*asset_id, vertex_slice, index_slice, blas_size))
        })
        .collect::<Vec<_>>();

    // Build geometry into each BLAS
    let build_entries = blas_resources
        .iter()
        .map(|(asset_id, vertex_slice, index_slice, blas_size)| {
            let geometry = BlasTriangleGeometry {
                size: blas_size,
                vertex_buffer: vertex_slice.buffer,
                first_vertex: vertex_slice.range.start,
                vertex_stride: 48,
                index_buffer: Some(index_slice.buffer),
                first_index: Some(index_slice.range.start),
                transform_buffer: None,
                transform_buffer_offset: None,
            };
            BlasBuildEntry {
                blas: &blas_manager.blas[asset_id],
                geometry: BlasGeometries::TriangleGeometries(vec![geometry]),
            }
        })
        .collect::<Vec<_>>();

    let mut command_encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("blas_build_command_encoder"),
    });
    let time_span = diagnostics
        .as_mut()
        .map(|diagnostics| diagnostics.time_span(&mut command_encoder, "blas_build"));
    command_encoder.build_acceleration_structures(&build_entries, &[]);
    if let Some(time_span) = time_span {
        time_span.end(&mut command_encoder);
    }
    render_queue.submit([command_encoder.finish()]);
}

pub fn compact_raytracing_blas(
    mut blas_manager: ResMut<BlasManager>,
    render_queue: Res<RenderQueue>,
) {
    let queue_size = blas_manager.compaction_queue.len();
    let mut meshes_processed = 0;
    let mut vertices_compacted = 0;

    while !blas_manager.compaction_queue.is_empty()
        && vertices_compacted < MAX_COMPACTION_VERTICES_PER_FRAME
        && meshes_processed < queue_size
    {
        meshes_processed += 1;

        let (mesh, vertex_count, compaction_started) =
            blas_manager.compaction_queue.pop_front().unwrap();

        let Some(blas) = blas_manager.get(&mesh) else {
            continue;
        };

        if !compaction_started {
            blas.prepare_compaction_async(|_| {});
        }

        if blas.ready_for_compaction() {
            let compacted_blas = render_queue.compact_blas(blas);
            blas_manager.insert(mesh, compacted_blas);

            vertices_compacted += vertex_count;
            continue;
        }

        // BLAS not ready for compaction, put back in queue
        blas_manager
            .compaction_queue
            .push_back((mesh, vertex_count, true));
    }
}

pub fn delete_raytracing_blas(
    mut blas_manager: ResMut<BlasManager>,
    render_queue: Res<RenderQueue>,
) {
    if blas_manager.pending_deletions.len() <= TLAS_BUILDS_BEFORE_DELETION_ALLOWED {
        return;
    }

    if let Some(deletable) = blas_manager
        .pending_deletions
        .pop_front()
        .filter(|b| !b.is_empty())
    {
        render_queue.on_submitted_work_done(move || drop(deletable));
    }
}

fn allocate_blas(
    vertex_slice: &MeshBufferSlice,
    index_slice: &MeshBufferSlice,
    asset_id: &AssetId<Mesh>,
    render_device: &RenderDevice,
) -> (Blas, BlasTriangleGeometrySizeDescriptor) {
    let blas_size = BlasTriangleGeometrySizeDescriptor {
        vertex_format: Mesh::ATTRIBUTE_POSITION.format,
        vertex_count: vertex_slice.range.len() as u32,
        index_format: Some(IndexFormat::Uint32),
        index_count: Some(index_slice.range.len() as u32),
        flags: AccelerationStructureGeometryFlags::OPAQUE,
    };

    // TODO: If we ever introduce BLAS refits, we need to be aware of the TLAS double-buffer
    // to avoid invalidating the previous frame TLAS
    let blas = render_device.wgpu_device().create_blas(
        &CreateBlasDescriptor {
            label: Some(&asset_id.to_string()),
            flags: AccelerationStructureFlags::PREFER_FAST_TRACE
                | AccelerationStructureFlags::ALLOW_COMPACTION,
            update_mode: AccelerationStructureUpdateMode::Build,
        },
        BlasGeometrySizeDescriptors::Triangles {
            descriptors: vec![blas_size.clone()],
        },
    );

    (blas, blas_size)
}

fn is_mesh_raytracing_compatible(mesh: &Mesh) -> bool {
    let triangle_list = mesh.primitive_topology() == PrimitiveTopology::TriangleList;
    let vertex_attributes = mesh
        .attributes()
        .map(|(attribute, _)| (attribute.id, attribute.format))
        .eq([
            (Mesh::ATTRIBUTE_POSITION.id, Mesh::ATTRIBUTE_POSITION.format),
            (Mesh::ATTRIBUTE_NORMAL.id, Mesh::ATTRIBUTE_NORMAL.format),
            (Mesh::ATTRIBUTE_UV_0.id, Mesh::ATTRIBUTE_UV_0.format),
            (Mesh::ATTRIBUTE_TANGENT.id, Mesh::ATTRIBUTE_TANGENT.format),
        ]);
    let indexed_32 = matches!(mesh.indices(), Some(Indices::U32(..)));
    mesh.enable_raytracing && triangle_list && vertex_attributes && indexed_32
}
