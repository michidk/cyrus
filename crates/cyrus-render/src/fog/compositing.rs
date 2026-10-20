//! Placed-flat submission order for destination-copy compositing.
use bevy::{
    pbr::Transmissive3d,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        render_phase::{ViewSortedRenderPhases, sort_phase_system},
        view::RetainedViewEntity,
    },
};
use cyrus_game::fidelity::Fidelity;
use std::collections::{HashMap, HashSet};

#[derive(Resource, Default)]
struct FlatOrder {
    known: HashSet<Entity>,
    ranks: HashMap<Entity, usize>,
}

#[derive(Resource, Default)]
struct SuppressedFlats(HashMap<RetainedViewEntity, Vec<Transmissive3d>>);

pub(super) fn register(app: &mut App) {
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .init_resource::<FlatOrder>()
        .init_resource::<SuppressedFlats>()
        .add_systems(Render, restore.before(RenderSystems::Queue))
        .add_systems(ExtractSchedule, extract)
        .add_systems(
            Render,
            order
                .after(sort_phase_system::<Transmissive3d>)
                .in_set(RenderSystems::PhaseSort),
        );
}

// Preservation model-lighting.md: the original partition is neither stable nor a depth sort.
fn partition<T>(items: &mut [(T, u16)]) {
    let count = items.iter().filter(|(_, alpha)| *alpha < 254).count();
    if count == 0 {
        return;
    }
    let mut tail = items.len() - 1;
    while tail > 0 && items[tail].1 < 254 {
        tail -= 1;
    }
    for front in 0..items.len().saturating_sub(count + 1) {
        if items[front].1 < 254 {
            items.swap(front, tail);
            tail = tail.saturating_sub(1);
            while tail > 0 && items[tail].1 < 254 {
                tail -= 1;
            }
        }
    }
}

// Preservation model-lighting.md: spatial lists use authored integer cells.
pub(super) fn selected_cell(position: Vec3, camera: Vec3) -> Option<[i32; 2]> {
    let origin = crate::lighting::runtime::region([
        (-f64::from(camera.x) * 20.0).floor() as i32,
        (f64::from(camera.z) * 20.0 + 65535.99609375).floor() as i32,
    ]);
    let x = (((-f64::from(position.x) * 20.0).round() as i32 & 65535) >> 9) - origin[0];
    let z =
        (((f64::from(position.z) * 20.0 + 65535.99609375).round() as i32 & 65535) >> 9) - origin[1];
    ((0..18).contains(&x) && (0..18).contains(&z) && !((x == 0 || x == 17) && (z == 0 || z == 17)))
        .then_some([x, z])
}

fn extract(
    mut order: ResMut<FlatOrder>,
    cameras: Extract<Query<(&GlobalTransform, &Projection), With<crate::camera::ViewCamera>>>,
    flats: Extract<
        Query<(
            Entity,
            &crate::flat::Flat,
            &GlobalTransform,
            &InheritedVisibility,
        )>,
    >,
    effects: Extract<Query<(Entity, &super::effects::EffectBillboard)>>,
    distance: Extract<Res<super::RenderDistance>>,
    fidelity: Extract<Res<Fidelity>>,
) {
    order.known.clear();
    order.ranks.clear();
    order.known.extend(flats.iter().map(|(entity, ..)| entity));
    order.known.extend(effects.iter().map(|(entity, _)| entity));
    let (Ok((camera, projection)), Some(clipping)) =
        (cameras.single(), distance.effective(&fidelity))
    else {
        return;
    };
    let projection = projection.get_clip_from_view();
    let scale = Vec2::new(projection.x_axis.x, projection.y_axis.y);
    let mut selected = Vec::new();
    for (entity, flat, transform, visibility) in &flats {
        if !visibility.get() {
            continue;
        }
        let position = transform.translation();
        let Some([x, z]) = selected_cell(position, camera.translation()) else {
            continue;
        };
        let center = camera.affine().inverse().transform_point3(position) * 20.0;
        if super::texture::admission::classify(
            [center.x * scale.x, center.y * scale.y, -center.z],
            flat.radius,
            scale,
            clipping,
        )
        .is_none()
        {
            continue;
        }
        selected.push((
            (z, x, std::cmp::Reverse(flat.source_index)),
            (entity, flat.alpha),
        ));
    }
    selected.sort_by_key(|(key, _)| *key);
    let mut entries: Vec<_> = selected.into_iter().map(|(_, entry)| entry).collect();
    let mut effect_entries: Vec<_> = effects
        .iter()
        .filter(|(_, effect)| effect.admitted)
        .collect();
    effect_entries.sort_by_key(|(_, effect)| effect.slot);
    entries.extend(
        effect_entries
            .into_iter()
            .map(|(entity, effect)| (entity, effect.alpha)),
    );
    partition(&mut entries);
    order.ranks.extend(
        entries
            .into_iter()
            .enumerate()
            .map(|(rank, (entity, _))| (entity, rank)),
    );
}

// Restore retained entries before Bevy applies removals and material respecialization.
fn restore(
    mut phases: ResMut<ViewSortedRenderPhases<Transmissive3d>>,
    mut suppressed: ResMut<SuppressedFlats>,
) {
    for (view, items) in suppressed.0.drain() {
        if let Some(phase) = phases.get_mut(&view) {
            for item in items {
                phase.add_retained(item);
            }
        }
    }
}

fn order(
    mut phases: ResMut<ViewSortedRenderPhases<Transmissive3d>>,
    order: Res<FlatOrder>,
    mut suppressed: ResMut<SuppressedFlats>,
) {
    for (view, phase) in phases.iter_mut() {
        let hidden: Vec<_> = phase
            .items
            .extract_if(.., |_, item| {
                let entity = item.entity.1.id();
                order.known.contains(&entity) && !order.ranks.contains_key(&entity)
            })
            .map(|(_, item)| item)
            .collect();
        if !hidden.is_empty() {
            suppressed.0.insert(*view, hidden);
        }
        phase.items.sort_by_key(|_, item| {
            order
                .ranks
                .get(&item.entity.1.id())
                .copied()
                .unwrap_or(usize::MAX)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extraction_appends_pool_slots_after_flats_before_shared_alpha_partition() {
        use super::super::effects::EffectBillboard;
        use bevy::{ecs::system::RunSystemOnce, render::MainWorld};
        let mut main = MainWorld::default();
        main.init_resource::<Fidelity>();
        main.insert_resource(super::super::RenderDistance {
            original: Some([1.0, 2000.0]),
            override_far: None,
        });
        let camera = Vec3::new(-230.4, 0.0, -3_046.4);
        main.spawn((
            crate::camera::ViewCamera,
            GlobalTransform::from_translation(camera),
            Projection::Perspective(PerspectiveProjection::default()),
        ));
        let flat = main
            .spawn((
                crate::flat::Flat {
                    brightness: Some(255),
                    alpha: 255,
                    source_index: 1,
                    radius: 1.0,
                },
                GlobalTransform::from_translation(camera - Vec3::Z * 10.0),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        // Spawn out of slot order and outside the placed-flat spatial window.
        let effect3 = main
            .spawn((
                EffectBillboard {
                    slot: 3,
                    alpha: 255,
                    admitted: true,
                },
                GlobalTransform::from_translation(Vec3::splat(1_000_000.0)),
            ))
            .id();
        let effect1 = main
            .spawn(EffectBillboard {
                slot: 1,
                alpha: 255,
                admitted: true,
            })
            .id();
        let effect2 = main
            .spawn(EffectBillboard {
                slot: 2,
                alpha: 255,
                admitted: true,
            })
            .id();
        let rejected = main
            .spawn(EffectBillboard {
                slot: 0,
                alpha: 255,
                admitted: false,
            })
            .id();
        let mut render = World::new();
        render.insert_resource(main);
        render.init_resource::<FlatOrder>();
        render.run_system_once(extract).unwrap();
        let ranks = &render.resource::<FlatOrder>().ranks;
        for (rank, entity) in [flat, effect1, effect2, effect3].into_iter().enumerate() {
            assert_eq!(ranks.get(&entity), Some(&rank));
        }
        assert!(render.resource::<FlatOrder>().known.contains(&rejected));
        assert!(!ranks.contains_key(&rejected));
        {
            let mut main = render.resource_mut::<MainWorld>();
            main.get_mut::<crate::flat::Flat>(flat).unwrap().alpha = 140;
            main.get_mut::<EffectBillboard>(effect3).unwrap().alpha = 140;
        }
        render.run_system_once(extract).unwrap();
        let ranks = &render.resource::<FlatOrder>().ranks;
        for (rank, entity) in [effect2, effect1, flat, effect3].into_iter().enumerate() {
            assert_eq!(ranks.get(&entity), Some(&rank));
        }
    }

    #[test]
    fn spatial_selection_excludes_corners_and_outside_cells() {
        let position = |x: i32, z: i32| {
            Vec3::new(
                -(x as f32) / 20.0,
                0.0,
                ((z as f64 - 65535.99609375) / 20.0) as f32,
            )
        };
        let camera = position(4608, 4608);
        for (cell, expected) in [
            ([0, 0], None),
            ([17, 17], None),
            ([0, 17], None),
            ([17, 0], None),
            ([1, 0], Some([1, 0])),
            ([17, 16], Some([17, 16])),
            ([18, 9], None),
            ([9, 18], None),
            ([9, 9], Some([9, 9])),
        ] {
            assert_eq!(
                selected_cell(position(cell[0] * 512, cell[1] * 512), camera),
                expected
            );
        }
    }

    #[test]
    fn retained_flat_returns_after_readmission_and_can_be_removed_while_excluded() {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::{
            core_pipeline::core_3d::TransparentSortingInfo3d,
            render::{
                render_phase::{DrawFunctionId, PhaseItemExtraIndex, SortedRenderPhase},
                render_resource::CachedRenderPipelineId,
                sync_world::MainEntity,
                view::RetainedViewEntity,
            },
        };
        let mut app = App::new();
        let entity = app.world_mut().spawn_empty().id();
        let main = MainEntity::from(entity);
        let view = RetainedViewEntity::new(main, None, 0);
        let mut phase = SortedRenderPhase::default();
        phase.add_retained(Transmissive3d {
            sorting_info: TransparentSortingInfo3d::AlwaysOnTop,
            distance: 0.0,
            pipeline: CachedRenderPipelineId::INVALID,
            entity: (entity, main),
            draw_function: DrawFunctionId(0),
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        });
        let mut phases = ViewSortedRenderPhases::default();
        phases.insert(view, phase);
        app.insert_resource(phases)
            .init_resource::<SuppressedFlats>()
            .add_systems(PreUpdate, restore)
            .insert_resource(FlatOrder {
                known: [entity].into(),
                ranks: HashMap::new(),
            })
            .add_systems(Update, order);
        app.update();
        assert!(
            app.world()
                .resource::<ViewSortedRenderPhases<Transmissive3d>>()[&view]
                .items
                .is_empty()
        );
        app.world_mut()
            .resource_mut::<FlatOrder>()
            .ranks
            .insert(entity, 0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<ViewSortedRenderPhases<Transmissive3d>>()[&view]
                .items
                .len(),
            1
        );
        app.world_mut().resource_mut::<FlatOrder>().ranks.clear();
        app.update();
        app.world_mut().run_system_once(restore).unwrap();
        app.world_mut()
            .resource_mut::<ViewSortedRenderPhases<Transmissive3d>>()
            .get_mut(&view)
            .unwrap()
            .remove(entity, main);
        app.world_mut()
            .resource_mut::<FlatOrder>()
            .ranks
            .insert(entity, 0);
        app.update();
        assert!(
            app.world()
                .resource::<ViewSortedRenderPhases<Transmissive3d>>()[&view]
                .items
                .is_empty()
        );
    }

    #[test]
    fn original_partition_retains_its_nonstable_boundary_behavior() {
        for (alpha, expected) in [
            (vec![181, 255], vec![0, 1]),
            (vec![181, 181, 255, 255], vec![3, 1, 2, 0]),
            (vec![255, 181, 255, 181], vec![0, 1, 2, 3]),
            (vec![181, 255, 255, 255], vec![3, 1, 2, 0]),
            (vec![181, 181, 181], vec![0, 1, 2]),
            (vec![254, 255, 256], vec![0, 1, 2]),
            (vec![], vec![]),
        ] {
            let mut entries: Vec<_> = alpha.into_iter().enumerate().collect();
            partition(&mut entries);
            assert_eq!(
                entries
                    .into_iter()
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
