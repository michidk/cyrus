//! Walking request ordering against the production animation interpreter.
use cyrus_assets::collision::CollisionGeometry;
use cyrus_assets::{
    animation::Animations,
    collision::{CollisionModel, CollisionScene, CollisionSubobject, StaticPlacement},
    player::MovementSettings,
};
use cyrus_collision::{
    collision_world::{BodyContact, StaticCandidates, StaticCollision},
    ground::{SupportHistory, TerrainGround},
};
use cyrus_game::player::{definition::PlayerDefinition, walk::WalkingStartup};

fn data(groups: serde_json::Value) -> (PlayerDefinition, Animations) {
    let metadata = serde_json::json!({
        "actors": [{"script_name":"CYRUS", "attributes":{
            "attr_forward_walk":8,"attr_forward_speed":24,"attr_step_type":1,"attr_panic_group":3
        },"mpsz_bounds_primary":0,"raex":{"unknown_12":256},"animation_groups":groups}],
        "mpsz_entries":[{"total_extent":[48,128,49],"pos_extent":[24,62,11],
            "neg_extent":[24,66,38],"center_offset":[0,0,0],"flags":0}]
    });
    let definition = PlayerDefinition::from_metadata(&metadata).unwrap();
    let animation = serde_json::from_value(serde_json::json!({
        "version":2,"world":1,"metadata":metadata,"sections":{},"models":{},"unsupported":{}
    }))
    .unwrap();
    (definition, animation)
}

#[test]
fn player_entry_preserves_selected_frame_and_does_not_start_the_ordinary_idle_default() {
    use cyrus_game::player::placement::initial_animation;
    let (_, data) = data(serde_json::json!([
        {"group_index":0,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":10},{"opcode":0,"frame_reference":11},
            {"opcode":0,"frame_reference":12},{"opcode":2,"value":0}
        ]},
        {"group_index":3,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":30},{"opcode":2,"value":0}
        ]}
    ]));
    for (ticks, next_frame) in [(0, 10), (1, 11), (2, 11)] {
        let (mut animation, frame) = initial_animation(&data, ticks, &mut |_| Ok(())).unwrap();
        assert_eq!(frame, Some(10));
        assert_eq!(animation.current_group(), Some(0));
        assert_eq!(animation.pending_group(), None);
        assert_eq!(animation.advance(1).unwrap(), Some(next_frame));
    }
}

#[test]
fn ordinary_default_replacement_retains_its_frame_when_the_next_stage_delays() {
    let scene = ground_scene(6);
    let (_, data) = data(serde_json::json!([
        {"group_index":0,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]},
        {"group_index":3,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":30},{"opcode":12,"value":2},
            {"opcode":0,"frame_reference":31},{"opcode":2,"value":0}]}
    ]));
    let mut animation =
        cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", 0).unwrap();
    animation.advance(1).unwrap();
    let mut history = SupportHistory::new(0, 0);
    assert_eq!(
        cyrus_game::player::grounded::advance_animation(
            &scene,
            1,
            &mut history,
            &mut animation,
            &mut |_| Ok(())
        )
        .unwrap(),
        Some(30)
    );
    assert_eq!(animation.default_group(), 3);
    assert_eq!(animation.current_group(), Some(3));
    assert_eq!(animation.pending_group(), None);
    assert_eq!(animation.advance(1).unwrap(), None);
}

#[test]
fn installed_player_entry_matches_original_initial_group_and_frame() {
    use cyrus_game::player::placement::initial_animation;
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (player entry animation)");
        return;
    };
    let path = std::fs::read_dir(install.maps())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("ISLAND.RGM")
        })
        .unwrap();
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path).unwrap()).unwrap();
    let data = Animations {
        version: 2,
        world: 1,
        metadata: rgpre::import::rgm::export_rgm_metadata_json(&map, None),
        sections: Default::default(),
        models: Default::default(),
        unsupported: Default::default(),
    };
    let (animation, frame) = initial_animation(&data, 1, &mut |_| Ok(())).unwrap();
    assert_eq!(animation.current_group(), Some(0));
    assert_eq!(animation.pending_group(), None);
    // Installed RAAN reference 396 selects CYRSA004 frame 1, matching the entry capture.
    assert_eq!(frame, Some(396));
}

#[test]
fn walking_request_reads_the_marker_after_immediate_animation_advance() {
    let (definition, data) = data(serde_json::json!([{
        "group_index":20,"anim_type":1,"anim_id":2,"commands":[
            {"opcode":5},{"opcode":4,"sound_param":7,"volume_shift":0},
            {"opcode":0,"frame_reference":0},{"opcode":0,"frame_reference":1},{"opcode":1}
        ]
    }]));
    let mut animation =
        cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", 0).unwrap();
    let mut walk = WalkingStartup::new(1).unwrap();
    let mut cues = Vec::new();
    let mut sounds = |cue| {
        cues.push(cue);
        Ok(())
    };
    let step = walk
        .request(&mut animation, &definition, 256, 0, &mut sounds)
        .unwrap()
        .unwrap();
    assert_eq!(step.frame, Some(0));
    assert_eq!(step.distance, 0);
    assert!(animation.translation_suppressed());
    for _ in 0..2 {
        animation.advance_with_sounds(1, &mut sounds).unwrap();
        assert!(animation.translation_suppressed());
    }
    animation.advance_with_sounds(1, &mut sounds).unwrap();
    assert!(!animation.translation_suppressed());
    let step = walk
        .request(&mut animation, &definition, 256, 0, &mut sounds)
        .unwrap()
        .unwrap();
    assert_eq!(step.distance, 682);
    assert_eq!(cues.len(), 1);
}

#[test]
fn pending_walking_group_does_not_advance_startup() {
    let (definition, data) = data(serde_json::json!([
        {"group_index":1,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":10},{"opcode":1}
        ]},
        {"group_index":20,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":20},{"opcode":1}
        ]}
    ]));
    let mut animation =
        cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", 0).unwrap();
    animation.request(1, 0, 0).unwrap();
    let mut walk = WalkingStartup::new(1).unwrap();
    assert!(
        walk.request(&mut animation, &definition, 256, 0, &mut |_| Ok(()))
            .unwrap()
            .is_none()
    );
    assert_eq!(animation.current_group(), Some(1));
    assert_eq!(animation.pending_group(), Some(20));
    animation.force_request(20, 0, 0).unwrap();
    let step = walk
        .request(&mut animation, &definition, 256, 0, &mut |_| Ok(()))
        .unwrap()
        .unwrap();
    assert_eq!(step.distance, 512);
}

#[test]
fn missing_walk_group_preserves_startup_and_zero_speed_uses_runtime_fallback() {
    let (definition, data) = data(serde_json::json!([]));
    let mut animation =
        cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", 0).unwrap();
    let mut walk = WalkingStartup::new(1).unwrap();
    assert!(
        walk.request(&mut animation, &definition, 256, 1, &mut |_| Ok(()))
            .unwrap()
            .is_none()
    );
    assert_eq!(walk.advance(0, 25, 256, true), 3);
    assert_eq!(walk.advance(0, 25, 256, true), 4);
    assert_eq!(walk.advance(0, 25, 256, true), 6);
    assert_eq!(walk.advance(0, 25, 256, true), 12);
    walk.finish_update(false);
    assert_eq!(walk.advance(0, 25, 128, true), 1);
}

fn ground_scene(surface: u8) -> cyrus_game::player::data::PlayerScene {
    use cyrus_game::player::data::PlayerScene;
    let (definition, _) = data(serde_json::json!([]));
    let collision = CollisionScene {
        texture_surfaces: Some(std::collections::BTreeMap::from([(
            (302 << 7) | 25,
            surface,
        )])),
        ..Default::default()
    };
    PlayerScene {
        source_identity: None,
        actor_collision: None,
        model_radii: Default::default(),
        world: 1,
        keyboard_codes: None,
        settings: MovementSettings {
            standing_height: -2,
            post_collide_height: 50,
            slide_range: -60000,
            slide_speed: None,
            turn_speed: 12,
            turn_max_speed: 48,
            poly_push_units: 16,
        },
        definition: Some(definition),
        terrain: Some(
            TerrainGround::new(
                vec![0; 256 * 256],
                vec![false; 256 * 256],
                Some(vec![25; 256 * 256]),
            )
            .unwrap(),
        ),
        static_collision: StaticCollision::new(&collision).unwrap(),
        collision,
        unavailable: None,
    }
}

#[test]
fn forward_query_commits_surface_state_before_admission() {
    use bevy::math::Mat3;
    use cyrus_game::player::grounded::{StaticGroundQuery, SurfaceUpdate};
    let candidates = StaticCandidates {
        faces: vec![],
        actors: vec![],
    };
    let position = [512 * 256, -64 * 256, 512 * 256];
    for (material, policy, collision_attribute, accepted, expected_surface, expected_veto) in [
        (6, SurfaceUpdate::Assign, 6, true, 6, 0),
        (11, SurfaceUpdate::Assign, 6, false, 11, 1),
        (6, SurfaceUpdate::QueryOnly, 6, false, 11, 1),
        (6, SurfaceUpdate::Excluded, 6, false, 11, 1),
        (6, SurfaceUpdate::Assign, 2, false, 11, 1),
    ] {
        let scene = ground_scene(material);
        let query = StaticGroundQuery {
            scene: &scene,
            candidates: &candidates,
            rotation: Mat3::IDENTITY,
            collision_attribute,
            alternate_collision: false,
            sidedness: true,
            surface_update: policy,
        };
        let mut history = SupportHistory::new(0, 0);
        let (mut surface, mut veto) = (11, 1);
        let support = query
            .forward_support(position, &mut history, &mut surface, &mut veto)
            .unwrap();
        assert_eq!(support.is_some(), accepted);
        assert_eq!((surface, veto), (expected_surface, expected_veto));
        if let Some(support) = support {
            assert_eq!(support.actor_height(62, -2), -64 * 256);
        }
    }
}

#[test]
fn support_miss_commits_history_and_clears_assigned_surface() {
    use bevy::math::Mat3;
    use cyrus_game::player::grounded::{StaticGroundQuery, SurfaceUpdate};
    let mut scene = ground_scene(6);
    scene.terrain = None;
    let candidates = StaticCandidates {
        faces: vec![],
        actors: vec![],
    };
    let query = StaticGroundQuery {
        scene: &scene,
        candidates: &candidates,
        rotation: Mat3::IDENTITY,
        collision_attribute: 4,
        alternate_collision: false,
        sidedness: true,
        surface_update: SurfaceUpdate::Assign,
    };
    let mut history = SupportHistory::new(1, 4);
    let (mut surface, mut veto) = (11, 1);
    assert!(
        query
            .support(
                [512 * 256, -64 * 256, 512 * 256],
                &mut history,
                &mut surface,
                &mut veto
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(history.ticks(), 12);
    assert_eq!((surface, veto), (0, 0));
}

#[test]
fn steep_forward_probe_stops_before_support_state_updates() {
    use bevy::math::Mat3;
    use cyrus_game::player::grounded::{StaticGroundQuery, SurfaceUpdate};
    let mut scene = ground_scene(6);
    // Raise the local slope without overflowing the original i16 grid.
    let mut heights = vec![0; 256 * 256];
    for row in 0..256 {
        for column in 0..4 {
            heights[row * 256 + column] = (column * 400) as i16;
        }
    }
    scene.terrain = Some(
        TerrainGround::new(heights, vec![false; 256 * 256], Some(vec![25; 256 * 256])).unwrap(),
    );
    let candidates = StaticCandidates {
        faces: vec![],
        actors: vec![],
    };
    let query = StaticGroundQuery {
        scene: &scene,
        candidates: &candidates,
        rotation: Mat3::IDENTITY,
        collision_attribute: 6,
        alternate_collision: false,
        sidedness: true,
        surface_update: SurfaceUpdate::Assign,
    };
    let mut history = SupportHistory::new(3, 4);
    let (mut surface, mut veto) = (11, 1);
    assert!(
        query
            .forward_support(
                [512 * 256, (800 - 62) * 256, 512 * 256],
                &mut history,
                &mut surface,
                &mut veto
            )
            .unwrap()
            .is_none()
    );
    assert_eq!((history.ticks(), surface, veto), (3, 11, 1));
}

#[test]
fn composed_walk_translates_only_after_support_and_animation_admission() {
    use bevy::math::Mat3;
    use cyrus_game::player::grounded::{
        ForwardWalk, GroundedWalk, StaticGroundQuery, SurfaceUpdate,
    };
    let candidates = StaticCandidates {
        faces: vec![],
        actors: vec![],
    };
    for (material, accepted) in [(6, true), (11, false)] {
        let scene = ground_scene(material);
        let query = StaticGroundQuery {
            scene: &scene,
            candidates: &candidates,
            rotation: Mat3::IDENTITY,
            collision_attribute: 6,
            alternate_collision: false,
            sidedness: true,
            surface_update: SurfaceUpdate::Assign,
        };
        let (_, data) = data(serde_json::json!([{
            "group_index":20,"anim_type":0,"anim_id":2,"commands":[
                {"opcode":4,"sound_param":7,"volume_shift":0},
                {"opcode":0,"frame_reference":0},{"opcode":1}
            ]
        }]));
        let mut animation =
            cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", 0).unwrap();
        let initial = [512 * 256, -64 * 256, 512 * 256];
        let mut walk = GroundedWalk {
            position: initial,
            surface: 0,
            startup: WalkingStartup::new(1).unwrap(),
        };
        let mut history = SupportHistory::new(0, 0);
        let mut veto = 0;
        let input = ForwardWalk {
            heading: 0,
            frame_scale: 256,
            nominal_ticks: 0,
        };
        let mut emitted = Vec::new();
        let step = walk
            .forward(
                &query,
                &input,
                (&mut history, &mut veto),
                &mut animation,
                &mut |sound, surface, position| {
                    emitted.push((sound, surface, position));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(step.is_some(), accepted);
        assert_eq!(
            walk.position,
            [
                initial[0],
                initial[1],
                initial[2] + if accepted { 512 } else { 0 }
            ]
        );
        assert_eq!(walk.surface, material);
        let expected = if accepted {
            vec![(
                cyrus_world::animation::AnimationSound::Play {
                    effect: 7,
                    rate_offset_hz: 0,
                },
                6,
                initial,
            )]
        } else {
            vec![]
        };
        assert_eq!(
            emitted, expected,
            "walking cue captures assigned surface and position before translation"
        );
        if accepted {
            let after_hook = walk.position;
            assert!(
                walk.continue_forward(&query, &input, (&mut history, &mut veto), &animation, true,)
                    .unwrap()
            );
            assert_eq!(
                walk.position, after_hook,
                "continuation must not repeat hook movement"
            );
            assert_eq!(walk.startup.advance(8, 24, 256, true), 682);
        } else {
            assert_eq!(walk.startup.advance(8, 24, 256, true), 512);
        }
    }
}

#[test]
fn grounded_response_assigns_height_after_translation() {
    use bevy::math::Mat3;
    use cyrus_game::player::grounded::{GroundedWalk, StaticGroundQuery, SurfaceUpdate};
    let scene = ground_scene(6);
    let candidates = StaticCandidates {
        faces: vec![],
        actors: vec![],
    };
    let query = StaticGroundQuery {
        scene: &scene,
        candidates: &candidates,
        rotation: Mat3::IDENTITY,
        collision_attribute: 6,
        alternate_collision: false,
        sidedness: true,
        surface_update: SurfaceUpdate::Assign,
    };
    let previous = [512 * 256, -60 * 256, 512 * 256];
    let mut walk = GroundedWalk {
        position: [previous[0], previous[1], previous[2] + 512],
        surface: 0,
        startup: WalkingStartup::new(1).unwrap(),
    };
    let mut heading = 0;
    let mut history = SupportHistory::new(0, 0);
    let mut veto = 0;
    walk.resolve_ground(&query, previous, &mut heading, &mut history, &mut veto)
        .unwrap();
    assert_eq!(walk.position, [previous[0], -64 * 256, previous[2] + 512]);
    assert_eq!((walk.surface, veto, heading), (6, 0, 0));
}

#[test]
fn body_normal_conversion_rounds_before_classification_without_normalizing() {
    use bevy::math::Vec3;

    let contact = BodyContact {
        model: 0,
        face: 0,
        point: Vec3::ZERO,
        normal: Vec3::new(0.5 / 256.0, -160.5 / 256.0, 1.5 / 256.0),
    };
    let converted = cyrus_game::player::contact::GroundedContact::from_body(&contact).unwrap();
    assert_eq!(converted.normal, [0, -160, 2]);
    assert_eq!(converted.classification, 2);
    let invalid = BodyContact {
        normal: Vec3::splat(f32::NAN),
        ..contact
    };
    assert!(cyrus_game::player::contact::GroundedContact::from_body(&invalid).is_err());
}

#[test]
fn stationary_grounded_update_separates_remaining_wall_overlap() {
    use bevy::math::Mat3;
    use cyrus_game::player::grounded::{GroundedWalk, StaticGroundQuery, SurfaceUpdate};
    let mut scene = ground_scene(6);
    scene.collision.models.insert(
        "wall".into(),
        CollisionModel {
            flags: 0,
            geometry: CollisionGeometry {
                version: Some(*b"v5.0"),
                radius: 300.0,

                vertices: vec![
                    [0.0, -200.0, -200.0],
                    [0.0, 200.0, -200.0],
                    [0.0, 200.0, 200.0],
                    [0.0, -200.0, 200.0],
                ],
                normals: vec![[1.0, 0.0, 0.0]],
                faces: vec![vec![0, 1, 2, 3]],
                textures: Some(vec![None]),
                subobjects: vec![CollisionSubobject {
                    center: [0.0; 3],
                    radius: 300.0,
                    extents: Some([1.0; 3]),
                    faces: vec![0],
                }],
            },
        },
    );
    scene.collision.placements.push(StaticPlacement {
        id: 0,
        model: "wall".into(),
        position: [500, 0, 512],
        rotation: [268435456, 0, 0, 0, 268435456, 0, 0, 0, 268435456],
    });
    scene.static_collision = StaticCollision::new(&scene.collision).unwrap();
    let previous = [512 * 256, -64 * 256, 512 * 256];
    let candidates = scene
        .static_collision
        .candidates([5000, 5000], previous, None)
        .unwrap();
    let query = StaticGroundQuery {
        scene: &scene,
        candidates: &candidates,
        rotation: Mat3::IDENTITY,
        collision_attribute: 6,
        alternate_collision: false,
        sidedness: true,
        surface_update: SurfaceUpdate::Assign,
    };
    let mut walk = GroundedWalk {
        position: previous,
        surface: 6,
        startup: WalkingStartup::new(1).unwrap(),
    };
    walk.resolve_ground(
        &query,
        previous,
        &mut 0,
        &mut SupportHistory::new(0, 0),
        &mut 0,
    )
    .unwrap();
    assert_eq!(
        walk.position,
        [previous[0] + 4096, previous[1], previous[2]]
    );
}

#[test]
fn residual_distance_rounds_before_map_unit_shift() {
    use cyrus_game::player::contact::residual_distance;
    assert_eq!(residual_distance([0; 3], [4095, 999, 64]).unwrap(), 4096);
    assert_eq!(residual_distance([0; 3], [4095, 999, 63]).unwrap(), 4095);
}

#[test]
fn admitted_update_continues_released_walk_until_idle_without_replacing_pending_default() {
    use bevy::math::Mat3;
    use cyrus_game::player::grounded::{
        ForwardWalk, GroundedUpdate, GroundedWalk, StaticGroundQuery, SurfaceUpdate,
    };
    let scene = ground_scene(6);
    let candidates = StaticCandidates {
        faces: vec![],
        actors: vec![],
    };
    let query = StaticGroundQuery {
        scene: &scene,
        candidates: &candidates,
        rotation: Mat3::IDENTITY,
        collision_attribute: 6,
        alternate_collision: false,
        sidedness: true,
        surface_update: SurfaceUpdate::Assign,
    };
    let (_, data) = data(serde_json::json!([
        {"group_index":3,"anim_type":0,"anim_id":2,"commands":[{"opcode":0,"frame_reference":10},{"opcode":1}]},
        {"group_index":20,"anim_type":0,"anim_id":2,"commands":[{"opcode":0,"frame_reference":20},{"opcode":1}]}
    ]));
    let mut animation =
        cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", 0).unwrap();
    let start = [512 * 256, -64 * 256, 512 * 256];
    let mut walk = GroundedWalk {
        position: start,
        surface: 6,
        startup: WalkingStartup::new(1).unwrap(),
    };
    let mut input = GroundedUpdate {
        motion: ForwardWalk {
            heading: 0,
            frame_scale: 256,
            nominal_ticks: 0,
        },
        walk: true,
    };
    let mut history = SupportHistory::new(0, 0);
    let mut veto = 0;
    cyrus_game::player::grounded::advance_animation(
        &scene,
        input.motion.nominal_ticks,
        &mut history,
        &mut animation,
        &mut |_| Ok(()),
    )
    .unwrap();
    let frame = walk
        .move_grounded(
            &query,
            &mut input,
            (&mut history, &mut veto),
            &mut animation,
            &mut |_, _, _| Ok(()),
        )
        .unwrap();
    assert_eq!(frame, Some(20));
    assert_eq!(walk.position, [start[0], start[1], start[2] + 512]);
    input.walk = false;
    cyrus_game::player::grounded::advance_animation(
        &scene,
        input.motion.nominal_ticks,
        &mut history,
        &mut animation,
        &mut |_| Ok(()),
    )
    .unwrap();
    walk.move_grounded(
        &query,
        &mut input,
        (&mut history, &mut veto),
        &mut animation,
        &mut |_, _, _| Ok(()),
    )
    .unwrap();
    assert_eq!(animation.pending_group(), Some(3));
    assert_eq!(animation.current_group(), Some(20));
    assert_eq!(walk.position, [start[0], start[1], start[2] + 512 + 682]);
    animation.force_request(3, 0, 0).unwrap();
    cyrus_game::player::grounded::advance_animation(
        &scene,
        input.motion.nominal_ticks,
        &mut history,
        &mut animation,
        &mut |_| Ok(()),
    )
    .unwrap();
    walk.move_grounded(
        &query,
        &mut input,
        (&mut history, &mut veto),
        &mut animation,
        &mut |_, _, _| Ok(()),
    )
    .unwrap();
    assert_eq!(walk.position, [start[0], start[1], start[2] + 512 + 682]);
    assert_eq!(walk.startup.advance(8, 24, 256, true), 512);
}

#[test]
fn released_marker_keeps_startup_and_support_veto_still_blocks_continuation() {
    use bevy::math::Mat3;
    use cyrus_game::player::grounded::{
        ForwardWalk, GroundedUpdate, GroundedWalk, StaticGroundQuery, SurfaceUpdate,
    };
    for (surface, expected_next) in [(6, 1024), (11, 512)] {
        let scene = ground_scene(surface);
        let candidates = StaticCandidates {
            faces: vec![],
            actors: vec![],
        };
        let query = StaticGroundQuery {
            scene: &scene,
            candidates: &candidates,
            rotation: Mat3::IDENTITY,
            collision_attribute: 6,
            alternate_collision: false,
            sidedness: true,
            surface_update: SurfaceUpdate::Assign,
        };
        let (_, data) = data(serde_json::json!([
            {"group_index":3,"anim_type":0,"anim_id":1,"commands":[
                {"opcode":0,"frame_reference":30},{"opcode":2,"value":0}
            ]},
            {"group_index":20,"anim_type":1,"anim_id":1,"commands":[
                {"opcode":5},{"opcode":0,"frame_reference":20},
                {"opcode":0,"frame_reference":21},{"opcode":1}
            ]}
        ]));
        let mut animation =
            cyrus_world::animation::ActorAnimation::from_asset(&data, "CYRUS", 3).unwrap();
        animation.request(20, 0, 0).unwrap();
        let start = [512 * 256, -64 * 256, 512 * 256];
        let mut walk = GroundedWalk {
            position: start,
            surface: 0,
            startup: WalkingStartup::new(1).unwrap(),
        };
        assert_eq!(walk.startup.advance(8, 24, 256, true), 512);
        let mut input = GroundedUpdate {
            motion: ForwardWalk {
                heading: 0,
                frame_scale: 256,
                nominal_ticks: 0,
            },
            walk: false,
        };
        let mut history = SupportHistory::new(0, 0);
        let ordinary_frame = cyrus_game::player::grounded::advance_animation(
            &scene,
            input.motion.nominal_ticks,
            &mut history,
            &mut animation,
            &mut |_| Ok(()),
        )
        .unwrap();
        let frame = walk
            .move_grounded(
                &query,
                &mut input,
                (&mut history, &mut 0),
                &mut animation,
                &mut |_, _, _| Ok(()),
            )
            .unwrap();
        assert_eq!(frame.or(ordinary_frame), Some(20));
        assert_eq!(walk.position, start);
        assert_eq!(animation.pending_group(), Some(3));
        assert!(animation.translation_suppressed());
        assert_eq!(walk.startup.advance(8, 24, 256, true), expected_next);
    }
}

#[test]
fn forward_probe_rotates_installed_extent_and_walk_distance_without_changing_height() {
    use bevy::math::{Mat3, Vec3};
    let (definition, _) = data(serde_json::json!([]));
    let position = [100, -777, 200];
    assert_eq!(
        definition.forward_probe(position, Mat3::IDENTITY).unwrap(),
        [100, -777, 5064]
    );
    let quarter_turn = cyrus_game::player::motion::upright_rotation(1536);
    assert_eq!(
        definition.forward_probe(position, quarter_turn).unwrap(),
        [4964, -777, 200]
    );
    let mut narrow = definition;
    narrow.positive_extent[2] = 0;
    narrow.forward_walk = 1;
    let ties = Mat3::from_cols(Vec3::X, Vec3::Y, Vec3::new(0.5 / 256.0, 1.0, 1.5 / 256.0));
    assert_eq!(
        narrow.forward_probe(position, ties).unwrap(),
        [100, -777, 202]
    );
    let negative_ties =
        Mat3::from_cols(Vec3::X, Vec3::Y, Vec3::new(-0.5 / 256.0, 1.0, -1.5 / 256.0));
    assert_eq!(
        narrow.forward_probe(position, negative_ties).unwrap(),
        [100, -777, 198]
    );
}

#[test]
fn runtime_steps_select_current_models_keep_multiple_ticks_and_clear_stale_poses() {
    use bevy::prelude::*;
    use cyrus_game::{
        AppState,
        fidelity::Fidelity,
        player::{
            Player, PlayerPlugin,
            data::{PlayerScene, PlayerSceneRequest},
            grounded::SurfaceUpdate,
            placement::{MarkerEntry, PlayerPose},
            runtime::{GroundedTick, PlayerRuntime},
        },
    };
    use cyrus_world::{
        animation::{AnimationModel, AnimationPose, AnimationRequest},
        sound::{SoundCue, SoundSurface},
    };
    let (_, mut animations) = data(serde_json::json!([
        {"group_index":0,"anim_type":0,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":0},{"opcode":0,"frame_reference":32},{"opcode":2,"value":1}
        ]},
        {"group_index":20,"anim_type":1,"anim_id":1,"commands":[
            {"opcode":4,"sound_param":7,"volume_shift":0},
            {"opcode":0,"frame_reference":32},{"opcode":0,"frame_reference":33},{"opcode":2,"value":0}
        ]},
        {"group_index":30,"anim_type":1,"anim_id":1,"commands":[
            {"opcode":0,"frame_reference":64},{"opcode":0,"frame_reference":65},{"opcode":1}
        ]}
    ]));
    animations.metadata["markers"] = serde_json::json!([
        {"map_position":[512,-64,512],"position":[0,0,0],"reserved":1}
    ]);
    animations.metadata["actors"][0]["raan_entries"] = serde_json::json!([
        {"file_path":"IDLE.3DC","frame_count":32},
        {"file_path":"WALK.3DC","frame_count":32},
        {"file_path":"SCRIPT.3DC","frame_count":32}
    ]);
    for model in ["IDLE", "WALK", "SCRIPT"] {
        animations.models.insert(
            model.into(),
            AnimationModel {
                radius: None,
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "animated_i32".into(),
                positions: vec![vec![]; 32],
                normals: vec![vec![]; 32],
            },
        );
    }
    let mut scene = ground_scene(6);
    scene.definition.as_mut().unwrap().ordinary_default_group = 0;
    // IDLE has no radius: selecting candidates before animation advances must fail.
    scene.model_radii.insert("WALK".into(), 100.0);
    scene.model_radii.insert("SCRIPT".into(), 100.0);
    let scripts = player_runtime_scripts(animations.world);
    animations.metadata["script_content_id"] = scripts.content_id.clone().into();
    let logical = cyrus_game::script::ScriptWorld::prepare(&scripts, 1).unwrap();
    let actor = logical.id(0).unwrap();
    let mut actors = cyrus_game::script::ScriptActors::default();
    actors.current = Some(logical);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Fidelity>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Assets<Animations>>()
        .init_resource::<Assets<PlayerScene>>()
        .add_message::<SoundCue>()
        .insert_resource(actors)
        .add_plugins(PlayerPlugin);
    let animation = app
        .world_mut()
        .resource_mut::<Assets<Animations>>()
        .add(animations);
    let source = app
        .world_mut()
        .resource_mut::<Assets<PlayerScene>>()
        .add(scene);
    app.insert_resource(AnimationRequest(animation.clone()))
        .insert_resource(PlayerSceneRequest(source.clone()));
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    assert!(app.world().get::<AnimationPose>(player).is_none());
    let tick = |nominal_ticks, rotation| GroundedTick {
        player,
        actor,
        entry: MarkerEntry::default(),
        source: source.clone(),
        animation: animation.clone(),
        nominal_ticks,
        frame_scale: 256,
        view: [512, 512],
        rotation,
        collision_attribute: 6,
        alternate_collision: false,
        sidedness: true,
        surface_update: SurfaceUpdate::Assign,
        walk: true,
    };
    for _ in 0..2 {
        app.world_mut()
            .resource_mut::<Messages<GroundedTick>>()
            .write(tick(1, Mat3::IDENTITY));
    }
    app.update();
    assert_eq!(app.world().resource::<PlayerRuntime>().error, None);
    let cues: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<SoundCue>>()
        .drain()
        .collect();
    assert_eq!(cues.iter().filter(|cue| matches!(cue, SoundCue::Animation { actor, sound: cyrus_world::animation::AnimationSound::Play { effect: 7, rate_offset_hz: 0 }, position: Some([131072, -16384, 131072]), .. } if *actor == player)).count(), 1);

    assert_eq!(
        app.world().get::<PlayerPose>(player).unwrap().position,
        [512 * 256, -64 * 256, 512 * 256 + 1194]
    );
    {
        let actors = app.world().resource::<cyrus_game::script::ScriptActors>();
        let logical = actors.current.as_ref().unwrap().spatial(actor).unwrap();
        assert_eq!(
            logical.local.position,
            [512 * 256, -64 * 256, 512 * 256 + 1194]
        );
        assert_eq!(logical.local.angles, [0, 0, 0]);
    }
    let pose = app.world().get::<AnimationPose>(player).unwrap();
    assert_eq!((pose.model.as_str(), pose.frame), ("WALK", 1));
    assert_eq!(
        app.world().get::<SoundSurface>(player),
        Some(&SoundSurface(6))
    );
    assert!(
        (app.world().get::<Transform>(player).unwrap().translation.z - 1194.0 / 5120.0).abs()
            < 1e-6
    );
    app.world_mut()
        .get_mut::<PlayerPose>(player)
        .unwrap()
        .heading = 1024;
    app.world_mut()
        .resource_mut::<Messages<GroundedTick>>()
        .write(tick(0, Mat3::IDENTITY));
    app.update();
    assert_eq!(
        app.world().get::<PlayerPose>(player).unwrap().position[2],
        512 * 256 + 2218
    );
    assert_eq!(app.world().get::<AnimationPose>(player).unwrap().frame, 1);
    assert_eq!(app.world().get::<PlayerPose>(player).unwrap().heading, 0);
    assert_eq!(
        app.world()
            .resource::<cyrus_game::script::ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .spatial(actor)
            .unwrap()
            .local
            .position,
        [512 * 256, -64 * 256, 512 * 256 + 2218]
    );
    app.world_mut()
        .resource_mut::<Messages<GroundedTick>>()
        .write(tick(1, Mat3::from_cols_array(&[f32::NAN; 9])));
    app.update();
    assert!(
        app.world()
            .resource::<PlayerRuntime>()
            .error
            .as_ref()
            .unwrap()
            .contains("nonfinite rotation")
    );
    app.world_mut()
        .resource_mut::<Messages<GroundedTick>>()
        .write(tick(1, Mat3::IDENTITY));
    app.update();
    assert_eq!(
        app.world().get::<PlayerPose>(player).unwrap().position[2],
        512 * 256 + 2218
    );
    app.world_mut().resource_mut::<MarkerEntry>().marker = 9;
    app.update();
    assert!(app.world().get::<PlayerPose>(player).is_none());
    assert!(app.world().get::<AnimationPose>(player).is_none());
    assert!(app.world().get::<SoundSurface>(player).is_none());
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    app.update();
    assert!(app.world().get_entity(player).is_err());
    app.world_mut().resource_mut::<MarkerEntry>().marker = 0;
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::InGame);
    app.update();
    let replacement = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    assert_ne!(replacement, player);
    app.world_mut()
        .resource_mut::<Messages<GroundedTick>>()
        .write(tick(1, Mat3::IDENTITY));
    app.update();
    assert_eq!(
        app.world().get::<PlayerPose>(replacement).unwrap().position,
        [512 * 256, -64 * 256, 512 * 256]
    );
    assert!(app.world().get::<AnimationPose>(replacement).is_none());

    // Replacing only the logical world must also reject queued old-generation ticks.
    let logical = cyrus_game::script::ScriptWorld::prepare(&scripts, 2).unwrap();
    let current = logical.id(0).unwrap();
    app.world_mut()
        .resource_mut::<cyrus_game::script::ScriptActors>()
        .current = Some(logical);
    let mut stale = tick(1, Mat3::IDENTITY);
    stale.player = replacement;
    app.world_mut()
        .resource_mut::<Messages<GroundedTick>>()
        .write(stale);
    app.update();
    assert!(app.world().get::<AnimationPose>(replacement).is_none());
    let admitted = || GroundedTick {
        player: replacement,
        actor: current,
        walk: false,
        ..tick(1, Mat3::IDENTITY)
    };
    app.world_mut()
        .resource_mut::<Messages<GroundedTick>>()
        .write(admitted());
    app.update();
    assert_eq!(app.world().resource::<PlayerRuntime>().error, None);
    {
        let mut actors = app
            .world_mut()
            .resource_mut::<cyrus_game::script::ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        world
            .advance(
                current,
                cyrus_game::script::ScriptStage::Body(cyrus_script::Thread::Main),
                cyrus_game::script::ScriptInputs {
                    nominal_ticks: Some(1),
                    frame_scale: Some(256),
                    ..Default::default()
                },
                Some(&mut |_, _| Ok(())),
                None,
                20,
            )
            .unwrap();
        assert_eq!(world.animation_frame(current).unwrap(), Some(64));
    }
    app.world_mut()
        .resource_mut::<Messages<GroundedTick>>()
        .write(GroundedTick {
            walk: true,
            ..admitted()
        });
    app.update();
    assert_eq!(app.world().resource::<PlayerRuntime>().error, None);
    let pose = app.world().get::<AnimationPose>(replacement).unwrap();
    assert_eq!((pose.model.as_str(), pose.frame), ("SCRIPT", 1));
    let actors = app.world().resource::<cyrus_game::script::ScriptActors>();
    assert_eq!(
        actors
            .current
            .as_ref()
            .unwrap()
            .animation_frame(current)
            .unwrap(),
        Some(65)
    );
    assert_eq!(
        app.world()
            .get::<cyrus_game::script::ScriptBinding>(replacement)
            .unwrap()
            .actor,
        current
    );
}

fn player_runtime_scripts(world: u32) -> cyrus_assets::script::Scripts {
    let mut scripts: cyrus_assets::script::Scripts = serde_json::from_value(serde_json::json!({
        "version":16,"combat_attacks":[],"dialogue":{"menu_start":[35,25],"max_menu_items":300,"max_dialog_lines":20,"max_text_width":500,"menu_traverse_delay":4,"print_text":true,"use_speech":true,"max_distance":1600,"rtx_filename":"ENGLISH.RTX"},"player_distance_override":false,"updates":{"task_system":true,"animation_system":true,"disable_slaves":false,"disable_master_slaves":false},"clock":{"normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,"use_smooth_fps":true,"use_smooth_divisor":true},"world":world,"actor_pool_capacity":10,"content_id":"",
        "definition":{"functions":[
            {"name":"NullFunction","param_count":0,"kind":"function"},
            {"name":"PushAnimation","param_count":2,"kind":"task"}
        ],"flags":[],"references":[],"attributes":[]},
        "actors":{"CYRUS":{"header_index":0,"code_origin":0,"instances":1,
            "namespace":"CYRUS","instance_variables":[[]],"location_offsets":[],"attributes":{},"native_hook":null,
            "program":{"version":3,"entry":0,"steps":{
                "0":{"next":1,"operation":{"type":"call","call":{
                    "function_id":1,"kind":"task","target":{"type":"me"},"function":"PushAnimation",
                    "arguments":[{"type":"integer","value":30},{"type":"integer","value":1}]
                }}},
                "1":{"next":2,"operation":{"type":"end","target":0}}
            }}
        }},"instances":[],"unsupported":{}
    }))
    .unwrap();
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}
