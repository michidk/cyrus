//! Installed player data, frame sources and the original walking release sequence.
use cyrus_assets::install::GameInstall;
use cyrus_game::player::data::PlayerScene;

#[test]
fn installed_scene_exports_original_player_and_ground_inputs() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let install = match GameInstall::open(&root) {
        Ok(install) => install,
        Err(error) => {
            eprintln!("skipped: no install (player export) ({error})");
            return;
        }
    };
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("player-export-test-{}", std::process::id()));
    let path = directory.join("ISLAND.glb");
    cyrus_assets::scene::export_scene(&install, "ISLAND", &path).unwrap();
    let scene =
        PlayerScene::decode(&std::fs::read(path.with_extension("player")).unwrap()).unwrap();
    assert!(scene.unavailable.is_none(), "{:?}", scene.unavailable);
    let keys = rgpre::import::keys_ini::KeysIni::parse(
        &std::fs::read_to_string(install.root.join("KEYS.INI")).unwrap(),
    )
    .unwrap();
    assert_eq!(scene.keyboard_codes, Some(keys.keyboard_codes()));
    assert_eq!(scene.settings.standing_height, -2);
    let mut pose = cyrus_game::player::placement::PlayerPose {
        position: [10_448_896, -178_944, 10_502_144],
        heading: 0,
    };
    cyrus_game::player::placement::settle_static(&scene, &mut pose).unwrap();
    assert_eq!(pose.position, [10_448_896, -190_464, 10_502_144]);
    assert_eq!(pose.heading, 0);
    assert_eq!(scene.collision.placements.len(), 1691);
    assert_eq!(scene.collision.models["PLANT000"].flags, 2);
    assert_eq!(scene.collision.models["BARREL"].flags, 1);
    assert!(scene.collision.unsupported.is_empty());
    let surfaces = scene.collision.texture_surfaces.as_ref().unwrap();
    let source = rgpre::import::surface_ini::SurfaceAssignments::parse(
        &std::fs::read_to_string(install.root.join("surface.ini")).unwrap(),
    )
    .unwrap();
    for key in 0..=u16::MAX {
        assert_eq!(
            surfaces.get(&key).copied().unwrap_or(0),
            source.surface(key)
        );
    }
    let material_position = [42008 * 256, 0, 40607 * 256];
    let material_support = cyrus_world::collision::ground::SelectedSupport::Primary {
        source: cyrus_world::collision::ground::SupportSource::Terrain,
        support: scene
            .terrain
            .as_ref()
            .unwrap()
            .sample(material_position)
            .unwrap(),
    };
    assert_eq!(
        scene
            .support_surface(&material_support, material_position)
            .unwrap(),
        6
    );
    let source_model = rgpre::import::model3d::parse_3d_file(
        &std::fs::read(install.root.join("fxart/CYRSA001.3DC")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        scene.model_radii["CYRSA001"],
        source_model.header.radius as f32 / 256.0
    );
    let position = [40816 * 256, -744 * 256, 41024 * 256];
    assert_eq!(
        scene
            .candidates([40816, 41024], position, Some("CYRSA001"))
            .unwrap(),
        scene
            .static_collision
            .candidates(
                [40816, 41024],
                position,
                Some(source_model.header.radius as f32 / 256.0)
            )
            .unwrap(),
    );
    assert!(
        scene
            .candidates([40816, 41024], position, Some("MISSING"))
            .unwrap_err()
            .to_string()
            .contains("radius absent")
    );
    let player = scene.definition.as_ref().unwrap();
    assert_eq!(player.forward_walk, 8);
    let terrain = scene.terrain.as_ref().unwrap();
    assert_eq!(
        terrain.texture([42008 * 256, 0, 40607 * 256]).unwrap(),
        Some(89)
    );
    assert_eq!(surfaces.get(&((302 << 7) | 25)), Some(&6));
    let support = terrain.sample([10_448_896, -178_944, 10_502_144]).unwrap();
    assert_eq!(support.height, -680.0);
    assert_eq!(support.normal, [0, -256, 0]);
    assert_eq!(
        support.actor_height(player.positive_extent[1], scene.settings.standing_height),
        -190_464
    );
    let animations: cyrus_assets::animation::Animations =
        serde_json::from_slice(&std::fs::read(path.with_extension("animations")).unwrap()).unwrap();
    verify_walking_release(&scene, &animations);
    for name in ["CYRSA001", "CYRSA012"] {
        let bytes = std::fs::read(
            path.with_extension("player-models")
                .join(format!("{name}.glb")),
        )
        .unwrap();
        let glb = bevy::gltf::gltf::Gltf::from_slice(&bytes).unwrap();
        let node = glb.nodes().find(|node| node.name() == Some(name)).unwrap();
        let mesh = node.mesh().unwrap();
        assert!(mesh.primitives().len() > 0);
        assert!(glb.textures().len() > 0);
        for primitive in mesh.primitives() {
            let extras: serde_json::Value =
                serde_json::from_str(primitive.extras().as_ref().unwrap().get()).unwrap();
            assert_eq!(extras["rgpre_geometry"]["version"], 1);
            let binding: cyrus_render::animation::MeshFrameBinding =
                serde_json::from_value(extras["rgpre_geometry"].clone()).unwrap();
            let base = cyrus_render::animation::FrameGeometry::from_model(
                &animations.models[name],
                0,
                &binding,
            )
            .unwrap();
            let positions: Vec<_> = primitive
                .reader(|_| glb.blob.as_deref())
                .read_positions()
                .unwrap()
                .collect();
            assert_eq!(positions, base.positions);
            assert!(
                cyrus_render::animation::FrameGeometry::from_model(
                    &animations.models[name],
                    1,
                    &binding
                )
                .is_ok()
            );
            assert!(
                !extras["rgpre_geometry"]["source_vertices"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

fn verify_walking_release(scene: &PlayerScene, data: &cyrus_assets::animation::Animations) {
    use cyrus_game::player::{
        grounded::{ForwardWalk, GroundedUpdate, GroundedWalk, StaticGroundQuery, SurfaceUpdate},
        motion::upright_rotation,
        walk::WalkingStartup,
    };
    use cyrus_world::{animation::ActorAnimation, collision::ground::SupportHistory};
    let mut animation = ActorAnimation::from_asset(data, "CYRUS", 0).unwrap();
    let mut reference = animation
        .request_with_sounds(20, 0, 1, &mut |_| Ok(()))
        .unwrap()
        .unwrap();
    for _ in 0..64 {
        let frame = data.resolve_frame("CYRUS", reference).unwrap();
        if frame.model == "CYRSA001" && frame.frame == 7 {
            break;
        }
        reference = animation
            .advance_with_sounds(1, &mut |_| Ok(()))
            .unwrap()
            .unwrap_or(reference);
    }
    let frame = data.resolve_frame("CYRUS", reference).unwrap();
    assert_eq!((frame.model.as_str(), frame.frame), ("CYRSA001", 7));
    let definition = scene.definition.as_ref().unwrap();
    let mut walk = GroundedWalk {
        position: [10_448_896, -190_464, 10_509_994],
        surface: 0,
        startup: WalkingStartup::new(definition.step_type).unwrap(),
    };
    for expected in [512, 682, 1024] {
        assert_eq!(
            walk.startup
                .advance(definition.forward_walk, definition.forward_speed, 256, true),
            expected
        );
    }
    let mut input = GroundedUpdate {
        motion: ForwardWalk {
            heading: 0,
            frame_scale: 256,
            nominal_ticks: 1,
        },
        walk: false,
    };
    let mut history = SupportHistory::new(0, 0);
    let mut veto = 0;
    let mut z = walk.position[2];
    for (group, model, pose, displacement, marker) in [
        (20, "CYRSA001", 8, 2048, false),
        (20, "CYRSA001", 9, 2048, false),
        (20, "CYRSA001", 10, 2048, false),
        (20, "CYRSA001", 11, 2048, false),
        (20, "CYRSA001", 12, 2048, false),
        (20, "CYRSA001", 26, 0, true),
        (20, "CYRSA001", 27, 0, true),
        (20, "CYRSA001", 28, 0, true),
        (20, "CYRSA001", 29, 0, true),
        (20, "CYRSA001", 30, 0, true),
        (20, "CYRSA001", 31, 0, true),
        (0, "CYRSA004", 1, 0, false),
    ] {
        reference = cyrus_game::player::grounded::advance_animation(
            scene,
            input.motion.nominal_ticks,
            &mut history,
            &mut animation,
            &mut |_| Ok(()),
        )
        .unwrap()
        .unwrap_or(reference);
        let active = data.resolve_frame("CYRUS", reference).unwrap();
        let candidates = scene
            .candidates([40798, 40975], walk.position, Some(&active.model))
            .unwrap();
        let query = StaticGroundQuery {
            scene,
            candidates: &candidates,
            rotation: upright_rotation(0),
            collision_attribute: 15,
            alternate_collision: false,
            sidedness: false,
            surface_update: SurfaceUpdate::Assign,
        };
        reference = walk
            .move_grounded(
                &query,
                &mut input,
                (&mut history, &mut veto),
                &mut animation,
                &mut |_| Ok(()),
            )
            .unwrap()
            .unwrap_or(reference);
        let frame = data.resolve_frame("CYRUS", reference).unwrap();
        z += displacement;
        assert_eq!((frame.model.as_str(), frame.frame), (model, pose));
        assert_eq!(animation.current_group(), Some(group));
        assert_eq!(animation.translation_suppressed(), marker);
        assert_eq!(walk.position, [10_448_896, -190_464, z], "pose {pose}");
        assert_eq!(
            animation.pending_group(),
            if group == 20 { Some(0) } else { None }
        );
    }
    assert_eq!(walk.startup.advance(8, 24, 256, true), 512);
}
