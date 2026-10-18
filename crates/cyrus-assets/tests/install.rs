#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! Needs an install: skips (and says so) when `redguard`/`$REDGUARD_DIR` is absent.

use anyhow::Context;
use cyrus_assets::install::GameInstall;
use cyrus_assets::loading::parse_one;
use rgpre::import::registry;

fn install() -> Option<GameInstall> {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    match GameInstall::open(&root) {
        Ok(i) => Some(i),
        Err(e) => {
            eprintln!("skipped: no install ({e})");
            None
        }
    }
}

#[test]
fn rejects_a_directory_that_is_not_an_install() {
    let err = GameInstall::open(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap_err()
        .to_string();
    assert!(err.contains("missing"), "{err}");
}

#[test]
fn every_scene_file_in_maps_parses() {
    let Some(install) = install() else { return };
    let registry = registry::scan_dir(install.maps()).unwrap();
    assert!(!registry.files.is_empty());
    let failed: Vec<_> = registry
        .files
        .values()
        .filter_map(|e| {
            parse_one(&e.path, e.file_type)
                .err()
                .map(|err| format!("{}: {err}", e.name))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn every_world_exports_valid_animation_geometry() {
    let Some(install) = install() else { return };
    let ini = rgpre::import::world_ini::find_world_ini(&install.root).unwrap();
    let worlds = rgpre::import::world_ini::WorldIni::parse(&std::fs::read_to_string(ini).unwrap());
    assert!(!worlds.entries.is_empty());
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("all-world-animations-test-{}", std::process::id()));
    let mut failures = Vec::new();
    for world in worlds.entries {
        let result = (|| -> anyhow::Result<()> {
            let path = output.join(format!("{}.glb", world.index));
            cyrus_assets::scene::export_scene(&install, &world.index.to_string(), &path)?;
            let animations: cyrus_assets::animation::Animations =
                serde_json::from_slice(&std::fs::read(path.with_extension("animations"))?)?;
            anyhow::ensure!(animations.world == world.index, "animation world mismatch");
            animations.validate()?;
            for actor in animations.metadata["actors"].as_array().unwrap() {
                let Some(groups) = actor.get("animation_groups") else {
                    continue;
                };
                let name = actor["script_name"].as_str().context("actor name absent")?;
                let sources: Vec<Option<usize>> = if actor["raan_entries"]
                    .as_array()
                    .is_some_and(|entries| !entries.is_empty())
                {
                    vec![None]
                } else {
                    animations.metadata["mpob_objects"]
                        .as_array()
                        .context("placed objects absent")?
                        .iter()
                        .enumerate()
                        .filter(|(_, placement)| {
                            placement["script_name"].as_str() == Some(name)
                                && placement["object_type"] == 1
                                && placement["is_static"] != 1
                                && placement["model_name"]
                                    .as_str()
                                    .is_some_and(|name| !name.is_empty())
                        })
                        .map(|(index, _)| Some(index))
                        .collect()
                };
                if sources.is_empty() {
                    eprintln!(
                        "world {} actor {name}: definition has groups but no supported instance frame source",
                        world.index
                    );
                }
                for group in groups
                    .as_array()
                    .context("animation groups are not an array")?
                {
                    for command in group["commands"]
                        .as_array()
                        .context("animation commands absent")?
                    {
                        if command["opcode"] == 0 {
                            let reference = u32::try_from(
                                command["frame_reference"]
                                    .as_u64()
                                    .context("frame reference absent")?,
                            )?;
                            for source in &sources {
                                match source {
                                    Some(index) => animations.resolve_map_frame(*index, reference),
                                    None => animations.resolve_frame(name, reference),
                                }
                                .with_context(|| {
                                    format!("actor {name} group {}", group["group_index"])
                                })?;
                            }
                        }
                    }
                }
            }
            anyhow::ensure!(
                animations.unsupported.is_empty(),
                "unsupported animation references: {:?}",
                animations.unsupported
            );
            eprintln!(
                "world {}: validated {} animation models, {} frames",
                world.index,
                animations.models.len(),
                animations
                    .models
                    .values()
                    .map(|model| model.positions.len())
                    .sum::<usize>()
            );
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("world {} ({}): {error:#}", world.index, world.map));
        }
        if output.exists() {
            std::fs::remove_dir_all(&output).unwrap();
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn world_exports_include_declared_terrain_and_textures() {
    let Some(install) = install() else { return };
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("world-export-test-{}", std::process::id()));
    for (name, terrain) in [
        ("START", true),
        ("PALACE", false),
        ("ISLAND", true),
        ("CATACOMB", false),
        ("BELLTOWR", false),
    ] {
        let path = output.join(format!("{name}.glb"));
        cyrus_assets::scene::export_scene(&install, name, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: serde_json::Value =
            serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
        let nodes = document["nodes"].as_array().unwrap();
        assert_eq!(nodes.iter().any(|node| node["name"] == "Terrain"), terrain);
        assert!(!document["images"].as_array().unwrap().is_empty());
        assert!(nodes.iter().any(|node| node.get("mesh").is_some()));
        for node in nodes.iter().filter(|node| node["name"] != "Terrain") {
            let Some(mesh) = node["mesh"].as_u64() else {
                continue;
            };
            let render = &node["extras"]["rgpre_render"];
            assert_eq!(render["version"], 1, "{name}: {}", node["name"]);
            let attributes = render
                .get("rob_attributes")
                .expect("ROB provenance is explicit");
            assert!(attributes.is_null() || attributes.as_u64().is_some_and(|flags| flags <= 255));
            for primitive in document["meshes"][mesh as usize]["primitives"]
                .as_array()
                .unwrap()
            {
                let lighting = &primitive["extras"]["rgpre_lighting"];
                assert_eq!(lighting["version"], 5, "{name}: {}", node["name"]);
                let position = primitive["attributes"]["POSITION"].as_u64().unwrap() as usize;
                assert_eq!(
                    lighting["vertices"].as_array().unwrap().len() as u64,
                    document["accessors"][position]["count"].as_u64().unwrap()
                );
            }
        }
        if name == "START" {
            // Installed BOAT2 source face 334 stores 6; original model preparation yields 24.
            let boat = nodes
                .iter()
                .find(|node| node["name"] == "B_020_SBOAT")
                .unwrap();
            let mesh = &document["meshes"][boat["mesh"].as_u64().unwrap() as usize];
            let mut checked = false;
            for primitive in mesh["primitives"].as_array().unwrap() {
                let extras = &primitive["extras"];
                for polygon in extras["rgpre_admission"]["polygons"].as_array().unwrap() {
                    if polygon["source_face"] != 334 {
                        continue;
                    }
                    for vertex in polygon["vertices"].as_array().unwrap() {
                        assert_eq!(
                            extras["rgpre_lighting"]["vertices"][vertex.as_u64().unwrap() as usize]
                                [0],
                            24
                        );
                    }
                    checked = true;
                }
            }
            assert!(checked, "START SBOAT deck face is present in export");
        }
        let environment: cyrus_assets::environment::Environment =
            serde_json::from_slice(&std::fs::read(path.with_extension("environment")).unwrap())
                .unwrap();
        assert_eq!(environment.sky.is_some(), terrain);
        assert_eq!(environment.water.is_some(), terrain);
        assert_eq!(environment.terrain_lighting.is_some(), terrain);
        assert_eq!(environment.terrain_heights.is_some(), terrain);
        environment.validate().unwrap();
        for name in environment.prelighting.instances.keys() {
            assert!(
                nodes
                    .iter()
                    .any(|node| node["name"].as_str() == Some(name.as_str())),
                "lighting placement missing from exported geometry: {name}"
            );
        }
        if name == "BELLTOWR" {
            assert_eq!(environment.object_lights.len(), 4);
            assert!(
                environment
                    .object_lights
                    .iter()
                    .any(|light| light.rgb[2] < 0)
            );
            assert!(
                environment
                    .object_lights
                    .iter()
                    .all(|light| light.radius > 0)
            );
            for required in ["object_lights", "texture_scale"] {
                let mut old = serde_json::to_value(&environment).unwrap();
                old.as_object_mut().unwrap().remove(required);
                assert!(
                    serde_json::from_value::<cyrus_assets::environment::Environment>(old).is_err()
                );
            }
            assert!(!environment.prelighting.instances.is_empty());
            assert!(
                environment
                    .prelighting
                    .lights
                    .iter()
                    .any(|light| light.rgb.iter().any(|v| *v < 0))
            );
        }

        if name == "ISLAND" {
            assert_eq!(environment.model_lighting.ambient, [64.0; 3]);
            assert_eq!(environment.model_lighting.directional, [255.0; 3]);
            let light = environment.terrain_lighting.as_ref().unwrap();
            assert_eq!(light.side, 256);
            assert_eq!(light.ambient, [64.0; 3]);
            assert_eq!(light.directional, [255.0; 3]);
            for (actual, expected) in
                light
                    .direction
                    .into_iter()
                    .zip([-0.49998432, -0.49998432, 0.7071289])
            {
                assert!((actual - expected).abs() < 0.000001);
            }
        }
        let animations: cyrus_assets::animation::Animations =
            serde_json::from_slice(&std::fs::read(path.with_extension("animations")).unwrap())
                .unwrap();
        animations.validate().unwrap();
        assert!(
            animations.unsupported.is_empty(),
            "{name}: {:?}",
            animations.unsupported
        );
        if name == "CATACOMB" {
            let rat = &animations.models["RAT"];
            assert_eq!(rat.encoding, "static_3d");
            assert_eq!(rat.positions.len(), 1);
            assert!(!rat.base_positions.is_empty());
        }
        let registry = registry::scan_dir(&install.root).unwrap();
        let map = registry
            .files
            .values()
            .filter(|entry| {
                entry.file_type == rgpre::import::FileType::Rgm
                    && entry.name.eq_ignore_ascii_case(name)
            })
            .min_by_key(|entry| registry.source_rank_key(&entry.path))
            .unwrap();
        let parsed_map =
            rgpre::import::rgm::parse_rgm_file(&std::fs::read(&map.path).unwrap()).unwrap();
        let original_header = parsed_map
            .sections
            .iter()
            .find_map(|section| match section {
                rgpre::import::rgm::RgmSection::Rahd(_, bytes) => Some(bytes),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            &animations.sections["RAHD"], original_header,
            "{name}: actor header fields were lost"
        );
        let (model_name, exported, source) = animations
            .models
            .iter()
            .find_map(|(name, model)| {
                if model.positions.len() < 2 {
                    return None;
                }
                registry
                    .files
                    .values()
                    .filter(|entry| {
                        entry.name.eq_ignore_ascii_case(name)
                            && matches!(entry.file_type, rgpre::import::FileType::Model3dc)
                    })
                    .min_by_key(|entry| registry.source_rank_key(&entry.path))
                    .map(|entry| (name, model, &entry.path))
            })
            .expect("scene must retain a referenced animated 3DC model");
        let parsed =
            rgpre::import::model3d::parse_3d_file(&std::fs::read(source).unwrap()).unwrap();
        assert_eq!(
            exported.positions.len(),
            parsed.header.num_frames as usize,
            "{model_name}"
        );
        for (frame, retained) in parsed.frame_vertex_data.iter().zip(&exported.positions) {
            let expected: Vec<_> = frame.coords.iter().map(|v| [v.x, v.y, v.z]).collect();
            assert_eq!(
                retained, &expected,
                "{model_name}: exported frame differs from parser"
            );
        }
        for (frame, retained) in parsed.frame_normal_data.iter().zip(&exported.normals) {
            let expected: Vec<_> = frame.face_normals.iter().map(|v| [v.x, v.y, v.z]).collect();
            assert_eq!(
                retained, &expected,
                "{model_name}: exported normals differ from parser"
            );
        }

        assert!(!animations.metadata["actors"].as_array().unwrap().is_empty());
        assert!(!animations.models.is_empty());
        assert!(
            animations
                .models
                .values()
                .any(|model| model.positions.len() > 1)
        );
        eprintln!(
            "{name}: {} animation models, {} unsupported references",
            animations.models.len(),
            animations.unsupported.len()
        );
        std::fs::remove_file(path.with_extension("animations")).unwrap();
        std::fs::remove_file(path.with_extension("environment")).unwrap();
        std::fs::remove_file(path.with_extension("player")).unwrap();
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn glide_sky_exports_use_their_own_palettes_and_plane_inputs() {
    let Some(install) = install() else { return };
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("sky-export-test-{}", std::process::id()));
    let mut skies = Vec::new();
    // Verified installed WORLD.INI and matching sky COL samples; no game bytes are fixtures.
    for (world, first_pixel, scale, background) in [
        ("1", [107, 181, 231, 255], 2000.0, [200, 190, 250]),
        ("27", [0, 8, 41, 255], 2000.0, [0, 0, 10]),
        ("28", [107, 181, 231, 255], 2000.0, [200, 100, 100]),
        ("6", [49, 33, 41, 255], 1500.0, [57, 49, 49]),
    ] {
        let path = output.join(format!("{world}.glb"));
        cyrus_assets::scene::export_scene(&install, world, &path).unwrap();
        let environment: cyrus_assets::environment::Environment =
            serde_json::from_slice(&std::fs::read(path.with_extension("environment")).unwrap())
                .unwrap();
        assert_eq!(environment.background, background, "world {world}");
        assert_eq!(environment.clipping, [7.0, 3800.0], "world {world}");
        assert_eq!(
            environment.fog_table[11],
            [38, 76, 136, 196],
            "world {world}"
        );
        assert_eq!(
            environment.fog_table[15],
            [255, 255, 255, 0],
            "world {world}"
        );
        let sky = environment.sky.unwrap();
        assert_eq!((sky.width, sky.height), (256, 256), "world {world}");
        assert_eq!(sky.level, -4500.0, "world {world}");
        assert_eq!(sky.scale, scale, "world {world}");
        assert_eq!(sky.speed, if world == "6" { 8 } else { 6 }, "world {world}");
        assert_eq!(&sky.rgba[..4], &first_pixel, "world {world}");
        if world == "1" || world == "28" {
            let sun = sky.sun.as_ref().expect("installed sun image");
            assert_eq!(sun.size, [256, 256]);
            assert_eq!(sun.half_size, [5120.0, 5120.0]);
            assert_eq!(sun.color, [255, 225, 100]);
            if world == "1" {
                assert_eq!(sun.center, [-11313.0, -11313.0, -16000.0]);
            }
            assert_eq!(sun.alpha.len(), 65536);
            assert_eq!(sun.alpha[0], 0);
            assert!(sun.alpha.contains(&255));
        } else {
            assert!(sky.sun.is_none());
        }
        assert!(
            sky.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
        skies.push(sky.rgba);
        for extension in ["environment", "animations", "sounds", "player", "glb"] {
            let file = path.with_extension(extension);
            if file.exists() {
                std::fs::remove_file(file).unwrap();
            }
        }
    }
    assert_ne!(
        skies[0], skies[1],
        "day and night must use different sky assets"
    );
    assert_ne!(
        skies[0], skies[2],
        "day and sunset must use different sky assets"
    );
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn water_exports_preserve_selected_vertices_and_world_wave_parameters() {
    let Some(install) = install() else { return };
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("water-export-test-{}", std::process::id()));
    // Verified installed WLD corner selections and WORLD.INI values.
    for (world, parameters, selected) in [
        ("1", [24.0, 28.0, 5.0], 49119),
        ("6", [24.0, 28.0, 5.0], 62050),
        ("17", [20.0, 18.0, 5.0], 64963),
    ] {
        let path = output.join(format!("{world}.glb"));
        cyrus_assets::scene::export_scene(&install, world, &path).unwrap();
        let environment: cyrus_assets::environment::Environment =
            serde_json::from_slice(&std::fs::read(path.with_extension("environment")).unwrap())
                .unwrap();
        environment.validate().unwrap();
        let heights = environment.terrain_heights.as_ref().unwrap();
        assert_eq!(heights.side, 256);
        assert_eq!(heights.heights.len(), 256 * 256);
        assert_eq!(heights.offset, 0);
        let mut missing = serde_json::to_value(&environment).unwrap();
        missing["terrain_heights"] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<cyrus_assets::environment::Environment>(missing)
                .unwrap()
                .validate()
                .is_err()
        );
        let water = environment.water.unwrap();
        assert_eq!(water.parameters, parameters, "world {world}");
        assert_eq!(water.side, 256);
        assert_eq!(
            water.animated_vertices.iter().filter(|&&v| v).count(),
            selected,
            "world {world}"
        );
        assert!(!water.animated_vertices[0]);
        assert!(!water.animated_vertices[256]);
        assert!(water.animated_vertices[257]);
        for extension in ["environment", "animations", "sounds", "player", "glb"] {
            let file = path.with_extension(extension);
            if file.exists() {
                std::fs::remove_file(file).unwrap();
            }
        }
    }
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn startup_export_uses_authored_animation_and_preserves_title_artwork() {
    let Some(install) = install() else { return };
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("startup-export-test-{}", std::process::id()));
    cyrus_assets::startup::export_startup(&install, &output).unwrap();
    let splash = output.join("startup");
    let image = |name: &str| image::open(splash.join(name)).unwrap().to_rgba8();
    let first = image("splash-00.png");
    let middle = image("splash-89.png");
    let last = image("splash-149.png");
    assert_eq!(middle.dimensions(), (640, 480));
    let brightness = |image: &image::RgbaImage| {
        image
            .pixels()
            .map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]))
            .sum::<u64>()
    };
    assert!(brightness(&middle) > brightness(&first));
    assert!(brightness(&middle) > brightness(&last));
    assert!(
        middle
            .pixels()
            .filter(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
            .count()
            > 10000
    );
    assert!((0..150).all(|frame| splash.join(format!("splash-{frame:02}.png")).is_file()));
    assert!(!splash.join("splash-150.png").exists());
    let title = image("title.png");
    let source = rgpre::import::gxa::parse_gxa_file(
        &std::fs::read(install.root.join("system/STARTUP2.GXA")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        &title.as_raw()[..640 * 467 * 4],
        &source.frames[0].rgba[..640 * 467 * 4]
    );
    assert!(
        title
            .enumerate_pixels()
            .any(|(x, y, p)| x < 150 && y >= 467 && p.0 == [32, 32, 32, 255])
    );
    assert!(
        title
            .enumerate_pixels()
            .any(|(x, y, p)| x > 500 && y >= 467 && p.0 == [50, 0, 0, 255])
    );
    for name in ["title-preparing.png", "title-menu.png", "title-sound.png"] {
        let stage = image(name);
        assert_eq!(stage.dimensions(), (640, 480));
        assert_eq!(
            &stage.as_raw()[..640 * 467 * 4],
            &title.as_raw()[..640 * 467 * 4]
        );
        assert!(
            stage
                .enumerate_pixels()
                .any(|(x, y, p)| { x > 400 && y >= 467 && p.0 == [50, 0, 0, 255] })
        );
        assert_ne!(
            stage, title,
            "{name} must replace the generic loading label"
        );
    }
    let modified = std::fs::metadata(splash.join("title.png"))
        .unwrap()
        .modified()
        .unwrap();
    cyrus_assets::startup::export_startup(&install, &output).unwrap();
    assert_eq!(
        std::fs::metadata(splash.join("title.png"))
            .unwrap()
            .modified()
            .unwrap(),
        modified
    );
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn hud_export_uses_installed_positions_items_and_artwork() {
    let Some(install) = install() else { return };
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("hud-export-test-{}", std::process::id()));
    cyrus_assets::hud_export::export_hud(&install, &output).unwrap();
    let hud = output.join("hud");
    let data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(hud.join("overlay.hud")).unwrap()).unwrap();
    assert_eq!(data["candle_mode"], 2);
    assert_eq!(data["candle"], serde_json::json!([12, 8]));
    assert_eq!(data["pickup"], serde_json::json!([12, 386]));
    assert_eq!(data["selected"], 1);
    let labels = data["labels"].as_object().unwrap();
    for (tag, notice) in [
        ("#wof", (0, 0)),
        ("#won", (0, 1)),
        ("#dof", (1, 0)),
        ("#don", (1, 1)),
    ] {
        let text = labels
            .iter()
            .find(|(label, _)| label.eq_ignore_ascii_case(tag))
            .unwrap()
            .1;
        assert_eq!(
            text, &data["notices"][notice.0][notice.1],
            "text-only RTX label {tag}"
        );
    }
    let items: Vec<_> = data["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["count"].as_u64().unwrap() > 0)
        .map(|i| {
            (
                i["id"].as_u64().unwrap(),
                i["bitmap"].as_u64().unwrap(),
                i["count"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(items, [(2, 26, 52), (4, 13, 5)]);
    assert_eq!(data["start_items"], serde_json::json!([1, 2, 4, 18]));
    assert_eq!(data["inventory"]["0"]["count"], 0);
    assert_eq!(data["inventory"]["0"]["player_max"], 1);
    assert_eq!(data["inventory"]["0"]["weapon"], false);
    assert_eq!(data["inventory"]["1"]["weapon"], true);
    assert_eq!(data["inventory"]["45"]["weapon"], false);
    assert!(
        data["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == 0)
    );
    let snuff = rgpre::import::gxa::parse_gxa_file(
        // Install file names vary in case; this one ships lowercase in GOG.
        &std::fs::read(
            std::fs::read_dir(install.root.join("system"))
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.file_name().unwrap().eq_ignore_ascii_case("SNUFF.GXA"))
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let candle = image::open(hud.join("candle-00.png")).unwrap().to_rgba8();
    assert_eq!(candle.as_raw(), &snuff.frames[0].rgba);
    assert!((0..81).all(|f| hud.join(format!("candle-{f:02}.png")).is_file()));
    assert!(hud.join("item-07.png").is_file());
    assert_eq!(data["text_alpha"], serde_json::json!([255, 199]));
    assert_eq!(data["player_text_alpha"], serde_json::json!([255, 199]));
    assert_eq!(data["font"]["line_height"], 26, "REDSEL.FNT");
    assert_eq!(data["prompt"], serde_json::json!([12, 436]));
    assert_eq!(data["override_ticks"], 24);
    assert_eq!(data["compass"], serde_json::json!([546, 396]));
    assert_eq!(data["compass_size"], serde_json::json!([73, 65]));
    assert_eq!(data["world_compass"]["3"], 2000);
    assert_eq!(data["world_compass"].as_object().unwrap().len(), 7);
    assert_eq!(
        data["powerup_slots"],
        serde_json::json!([[576, 6], [576, 96]])
    );
    assert_eq!(
        data["powerup_sizes"],
        serde_json::json!([[39, 61], [54, 61]])
    );
    assert_eq!(data["logbook"], serde_json::json!([540, 20]));
    assert_eq!(data["logbook_size"], serde_json::json!([73, 58]));
    assert_eq!(
        (&data["walk_mode"], &data["auto_defend"]),
        (&false.into(), &true.into())
    );
    let notices = data["notices"].as_array().unwrap();
    assert!(
        notices
            .iter()
            .flat_map(|n| n.as_array().unwrap())
            .all(|t| !t.as_str().unwrap().is_empty()),
        "RTX notice texts: {notices:?}"
    );
    for (name, count) in [("compass", 32), ("logbook", 32), ("powerup", 2)] {
        assert!((0..count).all(|f| hud.join(format!("{name}-{f:02}.png")).is_file()));
    }
    std::fs::remove_dir_all(&output).unwrap();
}
