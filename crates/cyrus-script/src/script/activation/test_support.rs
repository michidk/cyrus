//! Fixture access excluded from production builds.
use super::*;

/// State seeding and inspection for integration fixtures that cannot use ordinary stages.
/// Available only with the `test-support` feature; not a gameplay host interface.
pub trait GameplayGlobalsTestExt {
    /// Test hook: stand in for a save written before the ledge point moved into the actor.
    fn set_legacy_ledge_anchor_for_test(&mut self, point: [i32; 3], heading: u16);

    /// Test hook: establish selected-player health as the selected-player initializer does.
    fn initialize_selected_health_for_test(&mut self, health: u8);
}

impl GameplayGlobalsTestExt for GameplayGlobals {
    fn set_legacy_ledge_anchor_for_test(&mut self, point: [i32; 3], heading: u16) {
        self.legacy_ledge_anchor = Some((point, heading));
    }

    fn initialize_selected_health_for_test(&mut self, health: u8) {
        self.selected_health = Some(crate::script::SelectedHealthState::initialized(health));
    }
}
