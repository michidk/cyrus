//! Local book preferences, expressed in original menu action units.
use anyhow::ensure;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Toggle/volume preferences and bindings for original actions 710 through 724.
/// Missing local state uses the installed defaults exported by rgpre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuPreferences {
    pub values: BTreeMap<i32, i32>,
    pub bindings: [u8; 15],
}

impl MenuPreferences {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.values.len() == 5,
            "menu preferences: expected five settings"
        );
        for (action, max) in [(504, 1), (505, 1), (601, 20), (602, 20), (603, 1)] {
            ensure!(
                self.values
                    .get(&action)
                    .is_some_and(|v| (0..=max).contains(v)),
                "menu preferences: invalid action {action} value"
            );
        }
        ensure!(
            self.bindings.iter().all(|code| *code <= 139),
            "menu preferences: invalid binding code"
        );
        Ok(())
    }

    pub fn rebind(&mut self, index: usize, code: u8) {
        if let Some(conflict) = self.bindings.iter().position(|other| *other == code) {
            self.bindings[conflict] = 0;
        }
        self.bindings[index] = code;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reassignment_clears_first_conflict_and_preserves_other_controls() {
        let mut preferences = MenuPreferences {
            values: BTreeMap::new(),
            bindings: [0; 15],
        };
        preferences.bindings[0] = 17;
        preferences.bindings[1] = 31;
        preferences.rebind(1, 17);
        assert_eq!(&preferences.bindings[..3], &[0, 17, 0]);
        assert!(preferences.validate().is_err());
    }
}
