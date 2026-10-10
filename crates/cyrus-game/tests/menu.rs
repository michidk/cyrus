//! Installed-data contract: editing the local menu definition changes exported content.
#[cfg(unix)]
use cyrus_game::{install::GameInstall, startup::export_menu};
#[cfg(unix)]
use std::{path::PathBuf, time::SystemTime};

#[cfg(unix)]
#[test]
fn installed_menu_labels_change_without_changing_action_handlers() {
    let Ok(install) = GameInstall::locate(None) else {
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
    for name in ["fxart", "fonts", "SYSTEM.INI", "KEYS.INI"] {
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
        serde_json::from_slice(&std::fs::read(cache.join("menu/main.menu")).unwrap()).unwrap();
    let entries = menu["entries"].as_array().unwrap();
    let quit = entries.iter().find(|entry| entry["action"] == -2).unwrap();
    assert_eq!(quit["text"], "EXIT NOW");
    assert_eq!(quit["implemented"], true);
    assert!(
        entries
            .iter()
            .filter(|entry| entry["action"] != -2)
            .all(|entry| entry["implemented"] == false)
    );
    for name in menu["images"].as_array().unwrap() {
        let image = image::open(cache.join(name.as_str().unwrap())).unwrap();
        assert_eq!((image.width(), image.height()), (640, 480));
    }
    std::fs::remove_file(cache.join("menu/selection-0.png")).unwrap();
    export_menu(&edited, &cache).unwrap();
    assert!(cache.join("menu/selection-0.png").is_file());
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
