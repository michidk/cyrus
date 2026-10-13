use cyrus_assets::cheats::{self, CheatFile};
use cyrus_game::cheats::{CheatEffect, CheatStore, Cheats};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Store(Arc<Mutex<Option<Vec<u8>>>>);
impl CheatStore for Store {
    fn load(&self) -> anyhow::Result<CheatFile> {
        self.0
            .lock()
            .unwrap()
            .as_deref()
            .map(cheats::decode)
            .unwrap_or_else(|| Ok(cheats::empty()))
    }
    fn save(&self, state: &CheatFile) -> anyhow::Result<()> {
        *self.0.lock().unwrap() = Some(cheats::encode(state));
        Ok(())
    }
}

#[test]
fn toggles_assigns_lists_and_restores_only_after_savecheats() {
    let store = Store::default();
    let mut runtime = Cheats::new(store.clone());
    assert!(!runtime.distance_override().unwrap());
    assert_eq!(
        runtime.execute("neversaydie", None).unwrap(),
        CheatEffect::PlayerDistance
    );
    assert!(runtime.distance_override().unwrap());
    assert!(!Cheats::new(store.clone()).distance_override().unwrap());
    runtime.execute("neversaydie", Some(42)).unwrap();
    assert_eq!(runtime.state().unwrap().value("neversaydie"), Some(42));
    runtime.execute("savecheats", None).unwrap();
    let mut restored = Cheats::new(store);
    assert_eq!(restored.state().unwrap().value("neversaydie"), Some(42));
    let before = restored.inspect();
    assert_eq!(restored.execute("oracle", None).unwrap(), CheatEffect::List);
    assert_eq!(restored.inspect(), before);
    assert!(restored.execute("unknown", None).is_err());
    assert_eq!(restored.inspect(), before);
    restored.execute("neversaydie", None).unwrap();
    assert!(!restored.distance_override().unwrap());
    assert_eq!(
        restored.execute("oddjob", None).unwrap(),
        CheatEffect::NoEffect
    );
    assert_eq!(
        restored.execute("magiccarpet", None).unwrap(),
        CheatEffect::Unsupported
    );
}

#[test]
fn preserves_raw_values_and_unnamed_slots() {
    let mut bytes = vec![0; 256];
    bytes[252..].copy_from_slice(&[0x78, 0x56, 0x34, 0x12]);
    let store = Store(Arc::new(Mutex::new(Some(bytes.clone()))));
    let mut runtime = Cheats::new(store.clone());
    runtime.execute("oddjob", Some(u32::MAX)).unwrap();
    runtime.execute("savecheats", None).unwrap();
    bytes[44..48].copy_from_slice(&[255; 4]);
    assert_eq!(store.0.lock().unwrap().as_ref().unwrap(), &bytes);
}

#[test]
fn malformed_state_cannot_be_overwritten_by_commands() {
    let store = Store(Arc::new(Mutex::new(Some(vec![1, 2, 3]))));
    let mut runtime = Cheats::new(store.clone());
    assert_eq!(runtime.inspect()["ready"], false);
    assert!(runtime.execute("neversaydie", None).is_err());
    assert!(runtime.execute("savecheats", None).is_err());
    assert_eq!(store.0.lock().unwrap().as_deref(), Some(&[1, 2, 3][..]));
}

#[test]
fn failed_write_reports_error_and_retains_runtime_state() {
    struct ReadOnly;
    impl CheatStore for ReadOnly {
        fn load(&self) -> anyhow::Result<CheatFile> {
            Ok(cheats::empty())
        }
        fn save(&self, _: &CheatFile) -> anyhow::Result<()> {
            anyhow::bail!("disk full")
        }
    }
    let mut runtime = Cheats::new(ReadOnly);
    runtime.execute("neversaydie", None).unwrap();
    assert!(
        runtime
            .execute("savecheats", None)
            .unwrap_err()
            .to_string()
            .contains("disk full")
    );
    assert!(runtime.distance_override().unwrap());
}
