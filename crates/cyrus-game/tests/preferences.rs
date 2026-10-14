//! Persistence failure is observable and must not replace accepted runtime values.
use cyrus_assets::preferences::MenuPreferences;
use cyrus_game::preferences::{PreferenceKey, PreferenceStore, Preferences};
use std::sync::{Arc, Mutex};

struct Store {
    text: Result<Option<String>, &'static str>,
    fail_save: bool,
    writes: Arc<Mutex<Vec<String>>>,
}
impl PreferenceStore for Store {
    fn load(&self, _: PreferenceKey) -> anyhow::Result<Option<String>> {
        self.text.clone().map_err(anyhow::Error::msg)
    }
    fn save(&self, _: PreferenceKey, value: &str) -> anyhow::Result<()> {
        if self.fail_save {
            anyhow::bail!("disk full");
        }
        self.writes.lock().unwrap().push(value.into());
        Ok(())
    }
}
fn value(volume: i32) -> MenuPreferences {
    MenuPreferences {
        values: [(504, 1), (505, 0), (601, volume), (602, 10), (603, 1)].into(),
        bindings: [0; 15],
    }
}
#[test]
fn missing_state_stays_missing_and_save_commits_only_after_storage_accepts_it() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let mut state = Preferences::new(Store {
        text: Ok(None),
        fail_save: false,
        writes: writes.clone(),
    });
    assert_eq!(state.menu().clone(), Ok(None));
    state.save_menu(value(3)).unwrap();
    assert_eq!(state.menu().clone(), Ok(Some(value(3))));
    assert_eq!(
        serde_json::from_str::<MenuPreferences>(&writes.lock().unwrap()[0]).unwrap(),
        value(3)
    );
}
#[test]
fn failed_save_keeps_previous_values_and_malformed_load_cannot_be_overwritten() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let mut state = Preferences::new(Store {
        text: Ok(Some(serde_json::to_string(&value(2)).unwrap())),
        fail_save: true,
        writes: writes.clone(),
    });
    assert!(state.save_menu(value(9)).is_err());
    assert_eq!(state.menu().clone(), Ok(Some(value(2))));
    let mut malformed = Preferences::new(Store {
        text: Ok(Some("broken".into())),
        fail_save: false,
        writes: writes.clone(),
    });
    assert!(malformed.menu().is_err());
    assert!(malformed.save_menu(value(9)).is_err());
    assert!(writes.lock().unwrap().is_empty());
    assert!(Preferences::default().menu().is_err());
}
