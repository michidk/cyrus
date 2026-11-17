//! Native CHT restore and save in the installation's own `REDGUARD.CHT`, shared with the original game.
use anyhow::{Context, Result};
use bevy::prelude::{Commands, Res, error};
use cyrus_assets::{cheats::CheatFile, install::GameInstall};
use cyrus_game::cheats::{CheatStore, Cheats};

struct InstallCheats {
    install: GameInstall,
}
impl InstallCheats {
    fn path(&self) -> std::path::PathBuf {
        self.install.root.join("REDGUARD.CHT")
    }
}
impl CheatStore for InstallCheats {
    fn load(&self) -> Result<CheatFile> {
        let path = self.path();
        let mut state = match cyrus_config::read(&path)? {
            Some(bytes) => cyrus_assets::cheats::decode(&bytes)
                .with_context(|| format!("{}", path.display()))?,
            None => cyrus_assets::cheats::empty(),
        };
        let system = self.install.root.join("SYSTEM.INI");
        let text = std::fs::read_to_string(&system)
            .with_context(|| format!("reading {}", system.display()))?;
        cyrus_assets::cheats::CheatDefaults::parse(&text)?.apply(&mut state)?;
        Ok(state)
    }
    fn save(&self, state: &CheatFile) -> Result<()> {
        cyrus_config::write(&self.path(), &cyrus_assets::cheats::encode(state))
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
    commands.insert_resource(crate::console::markers(&install));
    let cheats = Cheats::new(InstallCheats {
        install: install.clone(),
    });
    if let Err(error) = cheats.state() {
        error!("cheat state unavailable: {error:#}");
    }
    commands.insert_resource(cheats);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
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
        fn store(&self) -> InstallCheats {
            InstallCheats {
                install: GameInstall {
                    root: self.0.join("install").canonicalize().unwrap(),
                },
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
    fn restores_and_saves_the_installs_own_file() {
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
        bytes[40] = 0;
        assert_eq!(std::fs::read(installed).unwrap(), bytes);
    }

    #[test]
    fn malformed_file_is_reported_and_not_overwritten() {
        let fixture = Fixture::new();
        let path = fixture.0.join("install/REDGUARD.CHT");
        std::fs::write(&path, b"bad").unwrap();
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
    fn missing_state_starts_empty() {
        let fixture = Fixture::new();
        assert!(!Cheats::new(fixture.store()).distance_override().unwrap());
    }
}
