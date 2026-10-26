//! The island map screen: the marker position rules and the screen lifecycle.
//!
//! Rules follow preservation's `docs/config/item-ini.md` "Island map screen". The screen opens from
//! the map input action or from using item 83, freezes the simulation like the inventory screen,
//! plays the cross animation once at 100 ms per frame and closes on the first key press that
//! arrives while the map key is up. Presentation draws [`IslandMapScreen`].
use anyhow::Context;

/// Binding slot of the map action among the 15 installed control codes (actions 718 to 724 are
/// inventory, sword, health, map, log, next, previous).
const MAP_SLOT: usize = 11;
/// The island map item, which the screen requires.
pub const MAP_ITEM: u16 = 83;
/// One cross frame lasts this long.
const FRAME_MILLISECONDS: u64 = 100;

/// Fixed Q8 marker positions of the worlds that do not follow the player.
const HOME_MARKER: [i32; 2] = [0x00C6_8000, 0x0079_3000];
const TOWER_MARKER: [i32; 2] = [0xFFE6_9C00_u32 as i32, 0x00B1_BC00];

/// What the screen reads from the live world when it opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IslandMapWorld {
    /// The current world number.
    pub world: u32,
    /// The selected player's local Q8 position, absent without a selected player.
    pub player: Option<[i32; 3]>,
    pub lockout: Option<u16>,
    /// Attribute 105 of the selected player.
    pub player_type: Option<u8>,
}

/// The marker's Q8 X and Z for `world`. Worlds 1, 6, 27, 28 and 30 follow the player, worlds 0, 7
/// and 14 and a missing player use fixed positions, and every other world keeps `previous`.
pub fn marker_position(world: &IslandMapWorld, previous: [i32; 2]) -> [i32; 2] {
    let Some(player) = world.player else {
        return HOME_MARKER;
    };
    match world.world {
        0 => HOME_MARKER,
        1 | 6 | 27 | 28 | 30 => [player[0], player[2]],
        7 | 14 => TOWER_MARKER,
        _ => previous,
    }
}

/// Screen position of the cross frame's top-left corner for a marker, given the map image width.
/// Each axis scales the map position (the Q8 value divided by 256, truncating) by
/// `width / 65535` and a per-world factor, truncates, and adds the world's offset; Z counts from
/// the south edge, `65536 - z`. The factors are single precision, as the original stores them.
pub fn cross_position(world: u32, marker: [i32; 2], width: u16) -> [i32; 2] {
    let (scale, offset) = if world == 6 {
        ([0x3f51_eb85, 0x3f07_ae14], [-86, 26])
    } else {
        ([0x3f61_47ae, 0x3f80_0000], [101, -27])
    };
    let unit = f64::from(width) / 65535.0;
    let axis = |map: i32, factor: u32| {
        let value = unit * f64::from(map) * f64::from(f32::from_bits(factor));
        (value as f32) as i32
    };
    [
        axis(marker[0] / 256, scale[0]) + offset[0],
        axis(65536 - marker[1] / 256, scale[1]) + offset[1],
    ]
}

/// Whether an open request is admitted: item 83 owned, the player lockout other than 1 and the
/// player's attribute 105 other than 1.
pub fn admitted(owns_map: bool, world: &IslandMapWorld) -> bool {
    owns_map && world.lockout != Some(1) && world.player_type != Some(1)
}

/// Where the screen is in its lifecycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub enum Phase {
    #[default]
    Closed,
    /// The cross animation, showing `frame`.
    Playing { frame: usize },
    /// The animation is done; the map key must be released before a key can close the screen.
    WaitingForRelease,
    /// Waiting for any key press while the map key is up.
    WaitingForKey,
}

/// The screen state shared with presentation and the input lock.
#[derive(bevy::prelude::Resource, Default, serde::Serialize)]
pub struct IslandMapScreen {
    pub phase: Phase,
    /// The cross frame's top-left corner in 640×480 screen coordinates.
    pub cross: [i32; 2],
    /// The marker's Q8 X and Z, kept between openings for worlds that do not set it.
    pub marker: [i32; 2],
    pub error: Option<String>,
    #[serde(skip)]
    started: Option<std::time::Duration>,
    #[serde(skip)]
    checkpoint: Option<crate::clock::ClockCheckpoint>,
    #[serde(skip)]
    paused_virtual_time: bool,
}

impl IslandMapScreen {
    /// Gameplay input, the world and the clock are held while the screen is up.
    pub fn open(&self) -> bool {
        self.phase != Phase::Closed
    }
}

pub(crate) fn register(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<IslandMapScreen>().add_systems(
        PreUpdate,
        (drive, publish_lock)
            .chain()
            .after(cyrus_console::ConsoleInput)
            .after(bevy::input::InputSystems),
    );
}

/// Suspension callers read the shared lock on the logbook screen.
fn publish_lock(
    screen: bevy::prelude::Res<IslandMapScreen>,
    mut logbook: Option<bevy::prelude::ResMut<crate::logbook::LogbookScreen>>,
) {
    if let Some(logbook) = logbook.as_mut()
        && logbook.map_open != screen.open()
    {
        logbook.map_open = screen.open();
    }
}

/// The installed island map artwork sizes, from the one loaded HUD asset.
fn artwork(
    assets: Option<&bevy::prelude::Assets<cyrus_assets::hud::HudAsset>>,
) -> Option<cyrus_assets::hud::IslandMapArtwork> {
    assets?.iter().next().map(|(_, hud)| hud.island_map.clone())
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Owns one shared input and clock transition; the tuple only groups optional resources past the system parameter limit.
fn drive(
    state: bevy::prelude::Res<bevy::prelude::State<crate::AppState>>,
    keys: bevy::prelude::Res<bevy::prelude::ButtonInput<bevy::prelude::KeyCode>>,
    time: bevy::prelude::Res<bevy::prelude::Time<bevy::prelude::Real>>,
    mut screen: bevy::prelude::ResMut<IslandMapScreen>,
    (hud, mut actors, logbook, inventory, console): (
        Option<bevy::prelude::Res<crate::script::ScriptHud>>,
        Option<bevy::prelude::ResMut<crate::script::ScriptActors>>,
        Option<bevy::prelude::Res<crate::logbook::LogbookScreen>>,
        Option<bevy::prelude::Res<crate::inventory_screen::InventoryScreen>>,
        Option<bevy::prelude::Res<cyrus_console::Console>>,
    ),
    (preferences, assets): (
        bevy::prelude::Res<crate::preferences::Preferences>,
        Option<bevy::prelude::Res<bevy::prelude::Assets<cyrus_assets::hud::HudAsset>>>,
    ),
    mut clock: Option<bevy::prelude::ResMut<crate::clock::SimulationClock>>,
    mut virtual_time: Option<bevy::prelude::ResMut<bevy::prelude::Time<bevy::prelude::Virtual>>>,
) {
    let tick = crate::clock::bios_tick(time.elapsed());
    let resume =
        |screen: &mut IslandMapScreen,
         virtual_time: &mut Option<
            bevy::prelude::ResMut<bevy::prelude::Time<bevy::prelude::Virtual>>,
        >,
         clock: &mut Option<bevy::prelude::ResMut<crate::clock::SimulationClock>>| {
            if screen.paused_virtual_time {
                if let Some(time) = virtual_time.as_mut() {
                    time.unpause();
                }
                screen.paused_virtual_time = false;
            }
            if let (Some(clock), Some(checkpoint)) = (clock.as_mut(), screen.checkpoint.take()) {
                clock.restore(checkpoint, tick);
            }
            screen.phase = Phase::Closed;
            screen.started = None;
        };
    // The request is consumed whether or not it is admitted, so a refusal is not retried.
    let requested = actors
        .as_mut()
        .and_then(|actors| actors.current.as_mut())
        .is_some_and(|world| world.globals_mut().take_island_map_request());
    if *state.get() != crate::AppState::InGame {
        resume(&mut screen, &mut virtual_time, &mut None);
        return;
    }
    let Some(bindings) = preferences
        .menu()
        .as_ref()
        .ok()
        .and_then(|p| p.as_ref())
        .map(|p| p.bindings)
        .or_else(|| logbook.as_ref().map(|screen| screen.bindings))
    else {
        return;
    };
    let map_key = crate::player::input::scan_key(bindings[MAP_SLOT]);
    let map_held = map_key.is_some_and(|key| keys.pressed(key));
    match screen.phase {
        Phase::Closed => {
            let pressed = map_key.is_some_and(|key| keys.just_pressed(key));
            if !(pressed || requested)
                || console.is_some_and(|console| console.open)
                || logbook.is_some_and(|screen| screen.suspended())
                || inventory.is_some_and(|screen| screen.suspended())
            {
                return;
            }
            let result = (|| {
                let hud = hud.as_ref().context("island map: HUD state unavailable")?;
                let world = actors
                    .as_ref()
                    .and_then(|actors| actors.current.as_ref())
                    .context("island map: no world")?;
                let inputs = world.island_map_world();
                let artwork = artwork(assets.as_deref())
                    .context("island map: installed artwork unavailable")?;
                Ok::<_, anyhow::Error>((
                    admitted(hud.item_owned(MAP_ITEM), &inputs),
                    inputs,
                    artwork,
                ))
            })();
            match result {
                Ok((true, inputs, artwork)) => {
                    screen.error = None;
                    screen.marker = marker_position(&inputs, screen.marker);
                    screen.cross = cross_position(inputs.world, screen.marker, artwork.map[0]);
                    if let Some(time) = virtual_time.as_mut()
                        && !time.is_paused()
                    {
                        time.pause();
                        screen.paused_virtual_time = true;
                    }
                    screen.checkpoint = clock.as_ref().map(|clock| clock.checkpoint());
                    screen.started = Some(time.elapsed());
                    screen.phase = Phase::Playing { frame: 0 };
                }
                Ok((false, ..)) => {}
                Err(error) => {
                    log::warn!("{error:#}");
                    screen.error = Some(format!("{error:#}"));
                }
            }
        }
        Phase::Playing { .. } => {
            let frames = artwork(assets.as_deref()).map_or(1, |art| art.cross.len().max(1));
            let elapsed = screen
                .started
                .map_or(0, |start| (time.elapsed() - start).as_millis() as u64);
            let frame = (elapsed / FRAME_MILLISECONDS) as usize;
            screen.phase = if frame >= frames {
                Phase::WaitingForRelease
            } else {
                Phase::Playing { frame }
            };
        }
        Phase::WaitingForRelease => {
            if !map_held {
                screen.phase = Phase::WaitingForKey;
            }
        }
        Phase::WaitingForKey => {
            if !map_held && keys.get_just_pressed().next().is_some() {
                resume(&mut screen, &mut virtual_time, &mut clock);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(world: u32, player: Option<[i32; 3]>) -> IslandMapWorld {
        IslandMapWorld {
            world,
            player,
            ..Default::default()
        }
    }

    #[test]
    fn marker_follows_the_player_in_five_worlds_and_is_fixed_or_kept_elsewhere() {
        let at = Some([100, 7, 200]);
        for number in [1, 6, 27, 28, 30] {
            assert_eq!(marker_position(&world(number, at), [9, 9]), [100, 200]);
        }
        assert_eq!(marker_position(&world(0, at), [9, 9]), HOME_MARKER);
        for number in [7, 14] {
            assert_eq!(marker_position(&world(number, at), [9, 9]), TOWER_MARKER);
        }
        assert_eq!(marker_position(&world(2, at), [9, 9]), [9, 9]);
        assert_eq!(marker_position(&world(6, None), [9, 9]), HOME_MARKER);
        assert_eq!(HOME_MARKER, [50816 * 256, 31024 * 256]);
        assert_eq!(TOWER_MARKER, [-6500 * 256, 45500 * 256]);
    }

    #[test]
    fn cross_position_scales_by_image_width_and_the_world_factors() {
        // 640 / 65535 per map unit, scale 0.88 in X and 1.0 in Z, offsets 101 and -27.
        assert_eq!(cross_position(1, [0, 65536 * 256], 640), [101, -27]);
        let [x, y] = cross_position(1, [50816 * 256, 31024 * 256], 640);
        assert_eq!(
            x,
            (640.0f64 / 65535.0 * 50816.0 * 0.88f32 as f64) as i32 + 101
        );
        assert_eq!(y, (640.0f64 / 65535.0 * (65536.0 - 31024.0)) as i32 - 27);
        // World 6 uses its own scales and offsets.
        let [x, y] = cross_position(6, [50816 * 256, 31024 * 256], 640);
        assert_eq!(
            x,
            (640.0f64 / 65535.0 * 50816.0 * 0.82f32 as f64) as i32 - 86
        );
        assert_eq!(
            y,
            (640.0f64 / 65535.0 * (65536.0 - 31024.0) * 0.53f32 as f64) as i32 + 26
        );
        // A negative X truncates toward zero.
        let [x, _] = cross_position(7, [-6500 * 256 - 100, 0], 640);
        assert_eq!(
            x,
            (640.0f64 / 65535.0 * -6500.0 * 0.88f32 as f64) as i32 + 101
        );
    }

    #[test]
    fn admission_needs_the_item_and_unlocked_player() {
        let open = IslandMapWorld {
            lockout: Some(0),
            player_type: Some(0),
            ..Default::default()
        };
        assert!(admitted(true, &open));
        assert!(!admitted(false, &open));
        assert!(!admitted(
            true,
            &IslandMapWorld {
                lockout: Some(1),
                ..open
            }
        ));
        assert!(!admitted(
            true,
            &IslandMapWorld {
                player_type: Some(1),
                ..open
            }
        ));
    }
}
