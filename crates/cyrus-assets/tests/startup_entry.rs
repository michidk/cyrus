#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
use cyrus_assets::{
    install::GameInstall,
    startup::{StartupEntry, startup_entry},
};

#[test]
fn installed_fresh_start_plays_introduction_and_valid_autosave_opens_book() {
    let install = match GameInstall::locate(None) {
        Ok(install) => install,
        Err(error) => return eprintln!("skipped: no install ({error})"),
    };
    assert_eq!(
        startup_entry(&install, false).unwrap(),
        StartupEntry::Introduction
    );
    assert_eq!(startup_entry(&install, true).unwrap(), StartupEntry::Book);
}

#[test]
fn installed_flags_override_autosave_and_report_invalid_configuration() {
    let root = std::env::temp_dir().join(format!("cyrus-startup-policy-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let install = GameInstall { root: root.clone() };
    for (final_version, ignore_autosave, valid, expected) in [
        (1, 0, false, StartupEntry::Introduction),
        (1, 0, true, StartupEntry::Book),
        (1, 1, true, StartupEntry::World),
        (1, 1, false, StartupEntry::World),
        (0, 0, true, StartupEntry::World),
        (0, 0, false, StartupEntry::World),
    ] {
        std::fs::write(
            root.join("SYSTEM.INI"),
            format!("[system]\n[debug]\nfinal_version={final_version}\n"),
        )
        .unwrap();
        std::fs::write(
            root.join("MENU.INI"),
            format!("[general]\nignore_autosave={ignore_autosave}\n"),
        )
        .unwrap();
        assert_eq!(startup_entry(&install, valid).unwrap(), expected);
        let source = cyrus_assets::native::Files::open(&install).unwrap();
        let prepared = cyrus_assets::game::prepare(&source, "startup/entry.json").unwrap();
        assert_eq!(prepared.len(), 1);
        let policy: cyrus_assets::startup::StartupPolicy =
            serde_json::from_slice(&prepared["startup/entry.json"]).unwrap();
        assert_eq!(policy.entry(valid), expected);
    }
    std::fs::write(
        root.join("MENU.INI"),
        "[general]\nignore_autosave=invalid\n",
    )
    .unwrap();
    assert!(
        startup_entry(&install, false)
            .unwrap_err()
            .to_string()
            .contains("ignore_autosave")
    );
    std::fs::remove_dir_all(root).unwrap();
}
