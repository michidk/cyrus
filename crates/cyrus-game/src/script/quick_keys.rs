//! The quick sword and quick health keys, sampled from the installed key bindings.
use bevy::prelude::*;

/// Binding slots of the quick sword and quick health keys among the 15 menu control codes.
const SWORD_SLOT: usize = 9;
const HEALTH_SLOT: usize = 10;

/// Quick keys for the ordinary update's key handler. The sword key reads as held; the health key
/// reads once per press, so a held key heals once.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct QuickKeys {
    pub sword: bool,
    pub health: bool,
}

/// Sample the keys under the same locks that suspend the player controller's input.
#[allow(clippy::too_many_arguments)] // Bevy supplies each independent input lock as a parameter.
pub(super) fn sample(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    suppressed: Option<Res<crate::player::input::InputSuppressed>>,
    pending_world: Option<Res<cyrus_world::lifecycle::WorldPreparationPending>>,
    logbook: Option<Res<crate::logbook::LogbookScreen>>,
    console: Option<Res<cyrus_console::Console>>,
    preferences: Option<Res<crate::preferences::Preferences>>,
    mut quick: ResMut<QuickKeys>,
) {
    let Some(keys) = keys else {
        *quick = QuickKeys::default();
        return;
    };
    if suppressed.is_some_and(|suppressed| suppressed.0)
        || pending_world.is_some()
        || logbook.as_ref().is_some_and(|screen| screen.suspended())
        || console.is_some_and(|console| console.open)
    {
        *quick = QuickKeys::default();
        return;
    }
    let bindings = preferences
        .as_ref()
        .and_then(|preferences| {
            preferences
                .menu()
                .as_ref()
                .ok()?
                .as_ref()
                .map(|menu| menu.bindings)
        })
        .or_else(|| logbook.as_ref().map(|screen| screen.bindings));
    let Some(bindings) = bindings else {
        *quick = QuickKeys::default();
        return;
    };
    let key = |slot: usize| crate::player::input::scan_key(bindings[slot]);
    quick.sword = key(SWORD_SLOT).is_some_and(|key| keys.pressed(key));
    quick.health = key(HEALTH_SLOT).is_some_and(|key| keys.just_pressed(key));
}
