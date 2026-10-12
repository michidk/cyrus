//! Installed receiver preparation retains special roles and ordinary authored allocations.
use cyrus_game::script::{ScriptActorSource, ScriptWorld};

#[test]
fn installed_maps_prepare_receivers_without_using_map_object_ids_as_runtime_ids() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP receiver instances)");
        return;
    }
    let mut maps = 0;
    for file in std::fs::read_dir(root.join("maps")).unwrap() {
        let path = file.unwrap().path();
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rgm"))
        {
            continue;
        }
        let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(&path).unwrap()).unwrap();
        let scripts = cyrus_assets::script_export::prepare(&map, &root, 0).unwrap();
        let world = ScriptWorld::prepare(&scripts, 1)
            .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
        for actor in world.actors() {
            if let Some(error) = actor.unavailable() {
                assert!(
                    !error.contains("authored instance allocation exceeded"),
                    "{}: {error}",
                    path.display()
                );
            }
            if let Some(placement) = actor.placement()
                && matches!(placement.object_type, 2 | 6)
            {
                assert!(actor.runtime_identifier.is_none());
                assert!(actor.runtime().is_none());
            }
        }
        for (name, role, identifier) in [
            ("CYRUS", "player", 1),
            ("GREMLIN", "alternate_player", 1),
            ("CAMERA", "camera", 2),
        ] {
            if let Some(definition) = scripts.actors.get(name) {
                let actor = world
                    .actors()
                    .iter()
                    .find(|actor| match actor.source {
                        ScriptActorSource::Player => role == "player",
                        ScriptActorSource::AlternatePlayer => role == "alternate_player",
                        ScriptActorSource::Camera => role == "camera",
                        _ => false,
                    })
                    .unwrap();
                assert_eq!(actor.runtime_identifier, Some(identifier));
                assert_eq!(actor.name(), name);
                assert!(actor.placement().is_none());
                assert_eq!(
                    actor.runtime().unwrap().variables(),
                    definition.instance_variables[0]
                );
            }
        }
        maps += 1;
    }
    assert!(maps > 0);
    eprintln!("SOUP receiver preparation: {maps} installed maps");
}

#[test]
fn installed_palace_nested_family_reconstructs_authored_poses() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard");
    if !root.join("RGFX.EXE").exists() {
        eprintln!("skipped: no install (SOUP nested family)");
        return;
    }
    let path = std::fs::read_dir(root.join("maps"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("PALACE.RGM")
        })
        .unwrap();
    let map = rgpre::import::rgm::parse_rgm_file(&std::fs::read(path).unwrap()).unwrap();
    let scripts = cyrus_assets::script_export::prepare(&map, &root, 3).unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    world.prepare_spatial().unwrap();
    let ids: Vec<_> = ["PI_PILA", "PI_PLTA", "PI_LVRA"]
        .into_iter()
        .map(|name| {
            world
                .id(world
                    .actors()
                    .iter()
                    .position(|actor| actor.name() == name)
                    .unwrap())
                .unwrap()
        })
        .collect();
    assert_eq!(world.spatial(ids[1]).unwrap().parent, Some(ids[0].index));
    assert_eq!(world.spatial(ids[2]).unwrap().parent, Some(ids[1].index));
    world.refresh_families().unwrap();
    for id in ids {
        let placement = world.actors()[id.index].placement().unwrap();
        let pose = world.spatial(id).unwrap().query_pose().unwrap();
        assert_eq!(
            pose.position,
            placement
                .map_position
                .map(|value| i32::try_from(value * 256).unwrap())
        );
        assert_eq!(
            pose.angles,
            placement.angles.map(|value| (value & 2047) as u16)
        );
    }
}
