//! Native console preferences and logs use local storage outside the install.
use anyhow::{Context, Result};
use cyrus_assets::install::GameInstall;
use cyrus_console::{Console, Preferences, Store};
use std::path::PathBuf;

struct LocalConsole {
    install: GameInstall,
    directory: PathBuf,
}
impl Store for LocalConsole {
    fn load(&self) -> Result<Option<Preferences>> {
        let local = self
            .install
            .cache_path(&self.directory)?
            .join("REDGUARD.CPF");
        for path in [local, self.install.root.join("REDGUARD.CPF")] {
            if let Some(bytes) = crate::storage::read(&path)? {
                return Preferences::decode(&bytes)
                    .with_context(|| format!("{}", path.display()))
                    .map(Some);
            }
        }
        Ok(None)
    }
    fn save(&self, preferences: &Preferences) -> Result<()> {
        crate::storage::write(
            &self
                .install
                .cache_path(&self.directory)?
                .join("REDGUARD.CPF"),
            &preferences.encode(),
        )
    }
    fn write_log(&self, text: &str) -> Result<()> {
        crate::storage::write(
            &self
                .install
                .cache_path(&self.directory)?
                .join("CONSOLE.LOG"),
            text.as_bytes(),
        )
    }
}

pub(crate) fn initial(install: &GameInstall) -> Console {
    Console::with_store(LocalConsole {
        install: install.clone(),
        directory: crate::settings::CONFIG_DIR.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpf_local_precedence_restart_and_malformed_protection_leave_install_unchanged() {
        let root = std::env::temp_dir().join(format!("cyrus-console-{}", std::process::id()));
        std::fs::create_dir_all(root.join("install")).unwrap();
        let store = LocalConsole {
            install: GameInstall {
                root: root.join("install").canonicalize().unwrap(),
            },
            directory: root.join("local"),
        };
        let installed = Preferences {
            font: 1,
            ..Default::default()
        }
        .encode();
        std::fs::write(store.install.root.join("REDGUARD.CPF"), &installed).unwrap();
        assert_eq!(store.load().unwrap().unwrap().font, 1);
        let local = Preferences {
            colors: [2, 3, 4, 5],
            ..Default::default()
        };
        store.save(&local).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), local);
        assert_eq!(
            std::fs::read(store.install.root.join("REDGUARD.CPF")).unwrap(),
            installed
        );
        let path = store.directory.join("REDGUARD.CPF");
        let retained = root.join("retained");
        std::fs::rename(&store.directory, &retained).unwrap();
        std::fs::write(&store.directory, b"blocked state directory").unwrap();
        assert!(store.save(&Preferences::default()).is_err());
        assert_eq!(
            std::fs::read(&store.directory).unwrap(),
            b"blocked state directory"
        );
        std::fs::remove_file(&store.directory).unwrap();
        std::fs::rename(&retained, &store.directory).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), local.encode());
        std::fs::write(&path, b"bad").unwrap();
        let mut app = bevy::prelude::App::new();
        app.add_plugins(cyrus_console::ConsolePlugin)
            .insert_resource(Console::with_store(store));
        assert!(
            cyrus_console::submit(
                app.world_mut(),
                "console save",
                cyrus_console::Source::Interactive
            )
            .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), b"bad");
        std::fs::remove_dir_all(root).unwrap();
    }
}
