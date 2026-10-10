//! Installed-data contract: editing the local menu definition changes exported content.
#[cfg(unix)]
use cyrus_game::{install::GameInstall, startup::export_menu};
#[cfg(unix)]
use std::{path::PathBuf, time::SystemTime};

#[cfg(unix)]
#[test]
fn installed_menu_labels_change_without_changing_action_handlers() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = GameInstall::open(&root) else {
        eprintln!("SKIP menu export: no Redguard install linked");
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
    // Only this temporary definition is writable; assets remain in the read-only install.
    #[cfg(unix)]
    for name in ["fxart", "fonts", "SYSTEM.INI", "KEYS.INI", "system"] {
        std::os::unix::fs::symlink(
            install.root.join(name).canonicalize().unwrap(),
            temp.join(name),
        )
        .unwrap();
    }
    let ini = std::fs::read_to_string(install.root.join("MENU.INI")).unwrap();
    let ini = ini.replace("QUIT", "EXIT NOW");
    std::fs::write(temp.join("MENU.INI"), &ini).unwrap();
    let cache = temp.join("cache");
    let edited = GameInstall { root: temp.clone() };
    export_menu(&edited, &cache).unwrap();
    let menu: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("menu/book.menu")).unwrap()).unwrap();
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
            .filter(|entry| ![-2, 3, 4].contains(&entry["item"]["action"].as_i64().unwrap()))
            .all(|entry| entry["item"]["implemented"] == false)
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
    assert_eq!(menu["defaults"]["values"]["601"], 20);
    assert_eq!(menu["defaults"]["values"]["602"], 16);
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
    std::fs::remove_file(cache.join("menu/book.menu")).unwrap();
    export_menu(&edited, &cache).unwrap();
    assert!(cache.join("menu/book.menu").is_file());
    std::fs::write(
        temp.join("MENU.INI"),
        ini.replace("texture[0] = 1", "texture[0] = 7"),
    )
    .unwrap();
    let error = export_menu(&edited, &cache).unwrap_err().to_string();
    assert!(error.contains("MENU.INI [page0] texture[0]"), "{error}");
    std::fs::remove_dir_all(temp).unwrap();
}

#[cfg(not(unix))]
#[test]
fn installed_menu_labels_change_without_changing_action_handlers() {
    eprintln!("SKIP menu export: temporary install symlinks require Unix");
}
