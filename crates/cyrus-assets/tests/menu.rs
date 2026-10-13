#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! Installed-data contract: editing the local menu definition changes exported content.
use cyrus_assets::install::GameInstall;
use cyrus_assets::startup::export_menu;
use std::{path::PathBuf, time::SystemTime};

#[test]
fn installed_menu_labels_change_without_changing_action_handlers() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("skipped: no install (menu export)");
        return;
    };
    let temp: PathBuf = std::env::temp_dir().join(format!(
        "cyrus-menu-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp).unwrap();
    // Copy inputs into a temporary manifest; only its local definitions are edited.
    for name in [
        "fxart",
        "fonts",
        "SYSTEM.INI",
        "KEYS.INI",
        "system",
        "sound",
    ] {
        for entry in walkdir::WalkDir::new(install.root.join(name)) {
            let entry = entry.unwrap();
            let target = temp.join(entry.path().strip_prefix(&install.root).unwrap());
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(target).unwrap();
            } else if entry.file_type().is_file() {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let ini = std::fs::read_to_string(install.root.join("MENU.INI")).unwrap();
    let ini = ini.replace("QUIT", "EXIT NOW");
    std::fs::write(temp.join("MENU.INI"), &ini).unwrap();
    let worlds = std::fs::read_to_string(install.root.join("WORLD.INI")).unwrap();
    std::fs::write(
        temp.join("WORLD.INI"),
        worlds
            .replace("start_world=0", "start_world=1")
            .replace("start_marker=0", "start_marker=3"),
    )
    .unwrap();
    let cache = temp.join("cache");
    let edited = GameInstall { root: temp.clone() };
    export_menu(&edited, &cache).unwrap();
    let sounds: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("sounds/bank.sounds")).unwrap()).unwrap();
    assert_eq!(sounds["effects"].as_array().unwrap().len(), 118);
    for effect in [13, 18, 64, 65, 66, 67, 68] {
        assert_eq!(sounds["effects"][effect]["base_volume"], 100);
        assert!(cache.join(format!("sounds/{effect:03}.wav")).is_file());
    }
    let menu: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
    assert_eq!(
        menu["clock"],
        serde_json::json!({
            "normal_frame_rate":12,"min_frame_rate":6,"max_frame_rate":300,
            "use_smooth_fps":true,"use_smooth_divisor":true,
        })
    );
    let mut invalid = menu.clone();
    invalid["clock"]["min_frame_rate"] = 0.into();
    assert!(
        serde_json::from_value::<cyrus_assets::menu::MenuAsset>(invalid)
            .unwrap()
            .validate()
            .is_err()
    );
    let mut old = menu.clone();
    old.as_object_mut().unwrap().remove("clock");
    assert!(serde_json::from_value::<cyrus_assets::menu::MenuAsset>(old).is_err());
    assert_eq!(menu["new_game"]["world"], 1);
    assert_eq!(menu["new_game"]["marker"], 3);
    assert_eq!(menu["new_game"]["loading"].as_array().unwrap().len(), 6);
    assert_eq!(menu["new_game"]["loading"][0]["width"], 640);
    assert_eq!(menu["new_game"]["loading"][0]["height"], 480);
    assert_eq!(menu["models"].as_array().unwrap().len(), 5);
    for (index, count) in [9, 9, 1, 9, 9].into_iter().enumerate() {
        assert_eq!(
            menu["models"][index]["frames"].as_array().unwrap().len(),
            count
        );
    }
    assert_ne!(
        menu["models"][1]["frames"][0],
        menu["models"][1]["frames"][4]
    );
    assert_ne!(
        menu["models"][1]["frames"][4],
        menu["models"][1]["frames"][8]
    );
    let entries = menu["pages"]["0"]["entries"].as_array().unwrap();
    let quit = entries
        .iter()
        .find(|entry| entry["item"]["action"] == -2)
        .unwrap();
    assert_eq!(quit["item"]["text"], "EXIT NOW");
    assert_eq!(quit["item"]["implemented"], true);
    assert!(
        entries
            .iter()
            .filter(|entry| ![-2, 2, 3, 4, 10].contains(&entry["item"]["action"].as_i64().unwrap()))
            .all(|entry| entry["item"]["implemented"] == false)
    );
    assert!(
        entries
            .iter()
            .filter(|entry| [2, 10].contains(&entry["item"]["action"].as_i64().unwrap()))
            .all(|entry| entry["item"]["implemented"] == true)
    );
    let display = menu["pages"]["5"]["entries"].as_array().unwrap();
    assert_eq!(
        display
            .iter()
            .map(|e| e["item"]["action"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [504, 505, 500]
    );
    assert_eq!(display[0]["output_position"], serde_json::json!([135, 160]));
    assert_eq!(menu["defaults"]["values"]["601"], 19);
    assert_eq!(menu["defaults"]["values"]["602"], 15);
    assert_eq!(menu["key_names"][72], "kbd up");
    assert_eq!(menu["pages"]["7"]["entries"][0]["item"]["action"], 710);
    assert_eq!(menu["pages"]["3"]["entries"].as_array().unwrap().len(), 6);
    assert_eq!(
        menu["pages"]["3"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["item"]["visible"] == true)
            .count(),
        2
    );
    std::fs::write(
        temp.join("MENU.INI"),
        ini.replace("force_movies = 0", "force_movies = 1"),
    )
    .unwrap();
    let keys = std::fs::read_to_string(install.root.join("KEYS.INI")).unwrap();
    std::fs::remove_file(temp.join("KEYS.INI")).unwrap();
    std::fs::write(
        temp.join("KEYS.INI"),
        keys.replace("HIT KEY", "PLEASE PRESS A KEY NOW"),
    )
    .unwrap();
    export_menu(&edited, &cache).unwrap();
    let forced: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
    assert!(
        forced["pages"]["3"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["item"]["visible"] == true)
    );
    assert_eq!(forced["waiting_message"], "PLEASE PRESS A ");
    let system = std::fs::read_to_string(temp.join("SYSTEM.INI")).unwrap();
    assert!(system.contains("normal_frame_rate=12"));
    std::fs::write(
        temp.join("SYSTEM.INI"),
        system.replace("normal_frame_rate=12", "normal_frame_rate=24"),
    )
    .unwrap();
    export_menu(&edited, &cache).unwrap();
    let changed: cyrus_assets::menu::MenuAsset =
        serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
    assert_eq!(changed.clock.normal_frame_rate, 24);
    std::fs::remove_file(cache.join("menu/book.menu")).unwrap();
    export_menu(&edited, &cache).unwrap();
    assert!(cache.join("menu/book.menu").is_file());
    std::fs::write(
        temp.join("WORLD.INI"),
        worlds.replace("start_world=0", "start_world=999999"),
    )
    .unwrap();
    assert!(
        export_menu(&edited, &cache)
            .unwrap_err()
            .to_string()
            .contains("start_world")
    );
    std::fs::write(temp.join("WORLD.INI"), &worlds).unwrap();
    std::fs::write(
        temp.join("MENU.INI"),
        ini.replace("texture[0] = 1", "texture[0] = 7"),
    )
    .unwrap();
    let error = export_menu(&edited, &cache).unwrap_err().to_string();
    assert!(error.contains("MENU.INI [page0] texture[0]"), "{error}");
    std::fs::remove_dir_all(temp).unwrap();
}
