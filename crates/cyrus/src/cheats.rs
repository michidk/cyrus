//! Native CHT restore and save outside the read-only installation.
use anyhow::{Context, Result};
use bevy::prelude::{Commands, Res, error};
use cyrus_assets::{cheats::CheatFile, install::GameInstall};
use cyrus_game::cheats::{CheatStore, Cheats};
use std::path::PathBuf;

struct LocalCheats {
    install: GameInstall,
    directory: PathBuf,
}
impl CheatStore for LocalCheats {
    fn load(&self) -> Result<CheatFile> {
        let local = self
            .install
            .cache_path(&self.directory)?
            .join("REDGUARD.CHT");
        let mut state = cyrus_assets::cheats::empty();
        for path in [local, self.install.root.join("REDGUARD.CHT")] {
            if let Some(bytes) = cyrus_config::read(&path)? {
                state = cyrus_assets::cheats::decode(&bytes)
                    .with_context(|| format!("{}", path.display()))?;
                break;
            }
        }
        let system = self.install.root.join("SYSTEM.INI");
        let text = std::fs::read_to_string(&system)
            .with_context(|| format!("reading {}", system.display()))?;
        cyrus_assets::cheats::CheatDefaults::parse(&text)?.apply(&mut state)?;
        Ok(state)
    }
    fn save(&self, state: &CheatFile) -> Result<()> {
        let local = self
            .install
            .cache_path(&self.directory)?
            .join("REDGUARD.CHT");
        cyrus_config::write(&local, &cyrus_assets::cheats::encode(state))
    }
}

pub(crate) fn load(install: Option<Res<GameInstall>>, mut commands: Commands) {
    let Some(install) = install else {
        return;
    };
    if let Err(error) = cyrus_assets::console::export(&install, cyrus_config::cache_dir()) {
        error!("console artwork unavailable: {error:#}");
    }
    commands.insert_resource(crate::console::initial(&install));
    let cheats = Cheats::new(LocalCheats {
        install: install.clone(),
        directory: cyrus_config::config_dir().into(),
    });
    if let Err(error) = cheats.state() {
        error!("cheat state unavailable: {error:#}");
    }
    commands.insert_resource(cheats);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "cyrus-cheats-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(path.join("install")).unwrap();
            std::fs::write(
                path.join("install/SYSTEM.INI"),
                "[system]\ntask_system=no\nanimation_system=no\n[debug]\nnode_marker=no\n",
            )
            .unwrap();
            Self(path)
        }
        fn store(&self) -> LocalCheats {
            LocalCheats {
                install: GameInstall {
                    root: self.0.join("install").canonicalize().unwrap(),
                },
                directory: self.0.join("local"),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn system_defaults_replace_only_the_three_startup_slots() {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.0.join("install/SYSTEM.INI"),
            "[system]\ntask_system=yes\nanimation_system=no\n[debug]\nnode_marker=yes\n",
        )
        .unwrap();
        let mut file = cyrus_assets::cheats::empty();
        for name in ["task", "animation", "nodemarker", "neversaydie"] {
            file.set(name, 42).unwrap();
        }
        fixture.store().save(&file).unwrap();
        let loaded = fixture.store().load().unwrap();
        assert_eq!(loaded.value("task"), Some(1));
        assert_eq!(loaded.value("animation"), Some(0));
        assert_eq!(loaded.value("nodemarker"), Some(1));
        assert_eq!(loaded.value("neversaydie"), Some(42));
    }

    #[test]
    fn restores_installed_state_but_saves_only_locally_and_prefers_local_on_restart() {
        let fixture = Fixture::new();
        let mut bytes = vec![0; 256];
        bytes[40] = 7;
        bytes[255] = 0x80;
        let installed = fixture.0.join("install/REDGUARD.CHT");
        std::fs::write(&installed, &bytes).unwrap();
        let mut runtime = Cheats::new(fixture.store());
        assert!(runtime.distance_override().unwrap());
        runtime.execute("neversaydie", Some(0)).unwrap();
        runtime.execute("savecheats", None).unwrap();
        assert!(!Cheats::new(fixture.store()).distance_override().unwrap());
        assert_eq!(std::fs::read(installed).unwrap(), bytes);
        bytes[40] = 0;
        assert_eq!(
            std::fs::read(fixture.0.join("local/REDGUARD.CHT")).unwrap(),
            bytes
        );
    }

    #[test]
    fn malformed_local_file_does_not_fall_back_to_install_or_get_overwritten() {
        let fixture = Fixture::new();
        std::fs::write(fixture.0.join("install/REDGUARD.CHT"), [0; 256]).unwrap();
        let path = fixture.0.join("local/REDGUARD.CHT");
        cyrus_config::write(&path, b"bad").unwrap();
        let mut runtime = Cheats::new(fixture.store());
        assert!(
            runtime
                .state()
                .unwrap_err()
                .to_string()
                .contains("REDGUARD.CHT")
        );
        assert!(runtime.execute("savecheats", None).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"bad");
    }

    #[test]
    fn failed_file_write_preserves_the_previous_local_document() {
        let fixture = Fixture::new();
        let mut runtime = Cheats::new(fixture.store());
        runtime.execute("savecheats", None).unwrap();
        let local = fixture.0.join("local");
        let retained = fixture.0.join("retained");
        std::fs::rename(&local, &retained).unwrap();
        std::fs::write(&local, b"blocked state directory").unwrap();
        runtime.execute("neversaydie", Some(3)).unwrap();
        assert!(runtime.execute("savecheats", None).is_err());
        assert!(runtime.distance_override().unwrap());
        assert_eq!(std::fs::read(&local).unwrap(), b"blocked state directory");
        std::fs::remove_file(&local).unwrap();
        std::fs::rename(&retained, &local).unwrap();
        assert_eq!(
            std::fs::read(fixture.0.join("local/REDGUARD.CHT")).unwrap(),
            vec![0; 256]
        );
    }

    #[test]
    fn missing_state_starts_empty_and_install_destination_is_rejected() {
        let fixture = Fixture::new();
        assert!(!Cheats::new(fixture.store()).distance_override().unwrap());
        let mut store = fixture.store();
        store.directory = store.install.root.clone();
        assert!(store.save(&cyrus_assets::cheats::empty()).is_err());
        assert!(!store.install.root.join("REDGUARD.CHT").exists());
    }
}
