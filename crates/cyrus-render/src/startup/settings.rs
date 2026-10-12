//! Persistent book preferences and their existing keyboard-input bindings.
use super::menu::MenuState;
use bevy::prelude::*;
use cyrus_assets::{menu::MenuAsset, preferences::MenuPreferences};

pub(super) fn persist(
    menu: &mut MenuState,
    preferences: MenuPreferences,
    store: &mut cyrus_game::preferences::Preferences,
) -> bool {
    if let Some(error) = &menu.persistence_error {
        menu.unavailable = Some(error.clone());
        return false;
    }
    match store.save_menu(preferences.clone()) {
        Ok(()) => {
            menu.preferences = Some(preferences);
            menu.unavailable = None;
            menu.dirty = true;
            true
        }
        Err(error) => {
            menu.unavailable = Some(format!("Settings were not saved: {error}"));
            false
        }
    }
}

pub(super) fn apply_bindings(
    menu: &MenuState,
    bindings: &mut cyrus_game::player::input::PlayerBindings,
) {
    if let Some(preferences) = &menu.preferences {
        match cyrus_game::player::input::PlayerBindings::from_codes(std::array::from_fn(|i| {
            preferences.bindings[i]
        })) {
            Ok(updated) => *bindings = updated,
            Err(error) => warn!("{error}; unsupported device bindings remain unavailable"),
        }
    }
}

pub(super) fn adjust_value(
    book: &MenuAsset,
    menu: &mut MenuState,
    direction: i32,
    toggle: bool,
    store: &mut cyrus_game::preferences::Preferences,
) -> bool {
    let Some(index) = menu.selected else {
        return false;
    };
    let entry = &book.pages[&menu.page].entries[index];
    let mut preferences = menu.preferences.clone().unwrap();
    let Some(value) = preferences.values.get_mut(&entry.item.action) else {
        return false;
    };
    let next = if toggle {
        1 - *value
    } else {
        (*value + direction).clamp(entry.slider[0], entry.slider[1])
    };
    if *value == next {
        return false;
    }
    *value = next;
    persist(menu, preferences, store)
}
