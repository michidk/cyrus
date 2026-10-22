//! Inventory screen list, cursor, layout and lifecycle, independent of screen presentation.
//! Rules follow preservation's `docs/engine/hud.md` inventory screen section.
use anyhow::Context;

/// Directions held during one screen update, after the host resolves installed bindings.
#[derive(Clone, Copy, Default)]
pub struct Directions {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
}

/// One drawn grid cell, top-left in 640×480 screen coordinates.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct Cell {
    pub id: u16,
    pub count: u32,
    pub at: [i32; 2],
    pub cursor: bool,
}

/// What one screen update draws besides the border.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Layout {
    pub cells: Vec<Cell>,
    pub up_arrow: bool,
    pub down_arrow: bool,
}

/// The four-column grid. The cursor and first visible row are kept between openings;
/// the original keeps both in static storage that starts at zero.
#[derive(Default)]
pub struct InventoryGrid {
    items: Vec<(u16, u32)>,
    cursor: i32,
    top: i32,
    delay: u8,
}

impl InventoryGrid {
    /// Start an opening with the listed items, in ID order, and the HUD selection.
    pub fn open(&mut self, items: Vec<(u16, u32)>, selected: Option<u16>) {
        if let Some(index) = selected.and_then(|id| items.iter().position(|item| item.0 == id)) {
            self.cursor = index as i32;
        }
        if items.is_empty() {
            self.cursor = -1;
        }
        self.items = items;
        self.delay = 0;
        self.scroll();
    }

    fn scroll(&mut self) {
        while self.cursor < self.top * 4 {
            self.top -= 1;
        }
        while self.top * 4 + 12 <= self.cursor {
            self.top += 1;
        }
    }

    /// One screen update. Returns whether the cursor moved, which plays sound effect 13.
    pub fn update(&mut self, held: Directions) -> bool {
        if !(held.left || held.right || held.up || held.down) || self.items.is_empty() {
            self.delay = 0;
            return false;
        }
        if self.delay > 0 {
            self.delay -= 1;
            return false;
        }
        let previous = self.cursor;
        let mut cursor = self.cursor;
        cursor += i32::from(held.right) - i32::from(held.left);
        cursor += 4 * (i32::from(held.down) - i32::from(held.up));
        self.cursor = cursor.min(self.items.len() as i32 - 1).max(0);
        self.scroll();
        let moved = self.cursor != previous;
        if moved {
            self.delay = 3;
        }
        moved
    }

    /// The item under the cursor, which Activate and Enter select.
    pub fn selection(&self) -> Option<u16> {
        usize::try_from(self.cursor)
            .ok()
            .and_then(|index| self.items.get(index))
            .map(|item| item.0)
    }

    /// Cells advance 105 pixels across from x = 100; a row ends past x = 519 and the next
    /// starts 102 pixels lower. Drawing stops after three visible rows.
    pub fn layout(&self) -> Layout {
        let mut layout = Layout {
            up_arrow: self.top != 0,
            ..Default::default()
        };
        let ([mut x, mut y], mut row) = ([100, 87], 0);
        for (index, &(id, count)) in self.items.iter().enumerate() {
            if self.top <= row {
                layout.cells.push(Cell {
                    id,
                    count,
                    at: [x, y],
                    cursor: index as i32 == self.cursor,
                });
            }
            x += 105;
            if x > 519 {
                x = 100;
                if self.top <= row {
                    y += 102;
                }
                row += 1;
                if row - self.top > 2 {
                    layout.down_arrow = self.items.len() as i32 > self.top * 4 + 12;
                    break;
                }
            }
        }
        layout
    }
}

/// Split an item name into lines: from byte 13, the first space found becomes a line break
/// and the search resumes 14 bytes later. The final byte is never replaced.
pub fn name_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let (mut start, mut index) = (0, 13);
    while index + 1 < bytes.len() {
        if bytes[index] == b' ' {
            lines.push(&text[start..index]);
            start = index + 1;
            index += 13;
        }
        index += 1;
    }
    lines.push(&text[start..]);
    lines
}

/// Binding slots among the 15 installed control codes.
const LEFT: usize = 2;
const RIGHT: usize = 3;
const UP: usize = 0;
const DOWN: usize = 1;
const WALK: usize = 4;
const ACTIVATE: usize = 7;
const INVENTORY: usize = 8;

/// Shared inventory-screen lifecycle. Presentation draws [`InventoryScreen::layout`].
/// Opening waits for the inventory key's release; closing keeps input suspended until
/// every exit control is released.
#[derive(bevy::prelude::Resource, Default, serde::Serialize)]
pub struct InventoryScreen {
    pub open: bool,
    pub waiting_for_release: bool,
    /// Gameplay keeps the opening key until it is released, before the first update.
    pub opening: bool,
    pub layout: Layout,
    pub error: Option<String>,
    #[serde(skip)]
    pub grid: InventoryGrid,
    #[serde(skip)]
    checkpoint: Option<crate::clock::ClockCheckpoint>,
    #[serde(skip)]
    last_tick: Option<u32>,
    #[serde(skip)]
    paused_virtual_time: bool,
}
impl InventoryScreen {
    /// Includes exit-key release, during which gameplay still must not consume input.
    pub fn suspended(&self) -> bool {
        self.open || self.waiting_for_release
    }
    /// The item under the cursor while the screen is open.
    pub fn cursor(&self) -> Option<u16> {
        self.open.then(|| self.grid.selection()).flatten()
    }
}
pub(crate) fn register(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<InventoryScreen>().add_systems(
        PreUpdate,
        (drive, publish_lock)
            .chain()
            .after(cyrus_console::ConsoleInput)
            .after(bevy::input::InputSystems),
    );
}

/// Suspension callers read the shared lock on the logbook screen.
fn publish_lock(
    screen: bevy::prelude::Res<InventoryScreen>,
    mut logbook: Option<bevy::prelude::ResMut<crate::logbook::LogbookScreen>>,
) {
    if let Some(logbook) = logbook.as_mut()
        && logbook.inventory_open != screen.suspended()
    {
        logbook.inventory_open = screen.suspended();
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // Owns one shared input and clock transition; the tuple only groups optional resources past the system parameter limit.
fn drive(
    state: bevy::prelude::Res<bevy::prelude::State<crate::AppState>>,
    keys: bevy::prelude::Res<bevy::prelude::ButtonInput<bevy::prelude::KeyCode>>,
    time: bevy::prelude::Res<bevy::prelude::Time<bevy::prelude::Real>>,
    mut screen: bevy::prelude::ResMut<InventoryScreen>,
    (hud, actors, logbook, console): (
        Option<bevy::prelude::Res<crate::script::ScriptHud>>,
        Option<bevy::prelude::Res<crate::script::ScriptActors>>,
        Option<bevy::prelude::Res<crate::logbook::LogbookScreen>>,
        Option<bevy::prelude::Res<cyrus_console::Console>>,
    ),
    preferences: bevy::prelude::Res<crate::preferences::Preferences>,
    mut clock: Option<bevy::prelude::ResMut<crate::clock::SimulationClock>>,
    mut virtual_time: Option<bevy::prelude::ResMut<bevy::prelude::Time<bevy::prelude::Virtual>>>,
    mut sounds: Option<bevy::prelude::MessageWriter<cyrus_world::sound::SoundCue>>,
) {
    use bevy::prelude::KeyCode;
    let tick = crate::clock::bios_tick(time.elapsed());
    let resume =
        |screen: &mut InventoryScreen,
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
        };
    if *state.get() != crate::AppState::InGame {
        resume(&mut screen, &mut virtual_time, &mut None);
        screen.open = false;
        screen.opening = false;
        screen.waiting_for_release = false;
        screen.checkpoint = None;
        screen.last_tick = None;
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
    let held = |slot: usize| {
        crate::player::input::scan_key(bindings[slot]).is_some_and(|key| keys.pressed(key))
    };
    let escape = keys.pressed(KeyCode::Escape);
    let enter = keys.pressed(KeyCode::Enter);
    if screen.waiting_for_release {
        if !(held(INVENTORY) || escape || held(WALK) || held(ACTIVATE) || enter) {
            screen.waiting_for_release = false;
            if let Some(hud) = hud.as_ref() {
                hud.set_inventory_candle(false);
            }
            resume(&mut screen, &mut virtual_time, &mut clock);
        }
        return;
    }
    if !screen.open {
        let opening = crate::player::input::scan_key(bindings[INVENTORY])
            .is_some_and(|key| keys.just_pressed(key));
        if !opening
            || console.is_some_and(|c| c.open)
            || logbook.is_some_and(|screen| screen.suspended())
        {
            return;
        }
        let result = (|| {
            let hud = hud.as_ref().context("inventory: HUD state unavailable")?;
            let (items, selected) = hud
                .inventory_list()
                .context("inventory: installed inventory data unavailable")?;
            // Attribute 105 value 1 empties the list.
            let world = actors.as_ref().and_then(|actors| actors.current.as_ref());
            let player_type = world
                .context("inventory: no world")?
                .player_activation_state()
                .ok()
                .and_then(|(id, _)| world?.actors().get(id.index).map(|a| a.attributes()[105]));
            let items = if player_type == Some(1) {
                Vec::new()
            } else {
                items
            };
            Ok::<_, anyhow::Error>((items, selected))
        })();
        match result {
            Ok((items, selected)) => {
                screen.error = None;
                screen.grid.open(items, selected);
                screen.layout = screen.grid.layout();
                if let Some(time) = virtual_time.as_mut()
                    && !time.is_paused()
                {
                    time.pause();
                    screen.paused_virtual_time = true;
                }
                if let Some(hud) = hud.as_ref() {
                    hud.set_inventory_candle(true);
                }
                screen.open = true;
                screen.opening = true;
                screen.last_tick = Some(tick);
                screen.checkpoint = clock.as_ref().map(|clock| clock.checkpoint());
            }
            Err(error) => {
                log::warn!("{error:#}");
                screen.error = Some(format!("{error:#}"));
            }
        }
        return;
    }
    if screen.opening {
        screen.opening = held(INVENTORY);
        return;
    }
    if screen.last_tick == Some(tick) {
        return;
    }
    screen.last_tick = Some(tick);
    let directions = Directions {
        left: held(LEFT),
        right: held(RIGHT),
        up: held(UP),
        down: held(DOWN),
    };
    if screen.grid.update(directions)
        && let Some(sounds) = sounds.as_mut()
    {
        sounds.write(cyrus_world::sound::SoundCue::Flat {
            effect: 13,
            volume: 0,
            rate_offset_hz: 0,
        });
    }
    screen.layout = screen.grid.layout();
    if held(INVENTORY) || held(ACTIVATE) || held(WALK) || escape || enter {
        if (held(ACTIVATE) || enter)
            && let (Some(id), Some(hud)) = (screen.grid.selection(), hud.as_ref())
            && let Err(error) = hud.select_item(id)
        {
            log::warn!("inventory: selecting item {id}: {error:#}");
        }
        screen.open = false;
        screen.waiting_for_release = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(count: u16) -> Vec<(u16, u32)> {
        (0..count).map(|id| (id, 1)).collect()
    }
    const RIGHT: Directions = Directions {
        right: true,
        left: false,
        up: false,
        down: false,
    };
    const DOWN: Directions = Directions {
        down: true,
        left: false,
        up: false,
        right: false,
    };

    #[test]
    fn cursor_starts_on_selection_and_keeps_its_value_otherwise() {
        let mut grid = InventoryGrid::default();
        grid.open(items(6), Some(4));
        assert_eq!(grid.selection(), Some(4));
        grid.open(vec![(1, 1), (2, 1), (3, 1), (9, 1), (10, 1)], Some(7));
        assert_eq!(grid.selection(), Some(10));
        grid.open(Vec::new(), Some(1));
        assert_eq!(grid.selection(), None);
        grid.open(items(3), Some(8));
        assert_eq!(
            grid.selection(),
            None,
            "an empty opening leaves the cursor at -1"
        );
    }

    #[test]
    fn moves_clamp_and_repeat_every_fourth_held_update() {
        let mut grid = InventoryGrid::default();
        grid.open(items(6), Some(0));
        let moves: Vec<_> = (0..9).map(|_| grid.update(RIGHT)).collect();
        assert_eq!(
            moves,
            [true, false, false, false, true, false, false, false, true]
        );
        assert_eq!(grid.selection(), Some(3));
        assert!(!grid.update(DOWN), "the delay holds any direction");
        assert_eq!(grid.selection(), Some(3));
        grid.update(Directions::default());
        assert!(grid.update(DOWN));
        assert_eq!(grid.selection(), Some(5), "down clamps to the last item");
        grid.update(Directions::default());
        assert!(
            !grid.update(DOWN),
            "a clamped move does not change the cursor"
        );
        assert!(!grid.update(DOWN), "and sets no delay");
    }

    #[test]
    fn layout_shows_three_rows_and_scroll_arrows() {
        let mut grid = InventoryGrid::default();
        grid.open(items(14), Some(0));
        let layout = grid.layout();
        assert_eq!(layout.cells.len(), 12);
        assert_eq!(layout.cells[0].at, [100, 87]);
        assert_eq!(layout.cells[3].at, [415, 87]);
        assert_eq!(layout.cells[4].at, [100, 189]);
        assert_eq!(layout.cells[11].at, [415, 291]);
        assert!(layout.cells[0].cursor);
        assert!(!layout.up_arrow && layout.down_arrow);

        grid.open(items(14), Some(13));
        let layout = grid.layout();
        assert_eq!(
            layout.cells.first().map(|c| (c.id, c.at)),
            Some((4, [100, 87]))
        );
        assert_eq!(layout.cells.len(), 10);
        assert!(layout.up_arrow && !layout.down_arrow);
        assert!(layout.cells.iter().any(|c| c.id == 13 && c.cursor));

        grid.open(items(12), Some(0));
        let layout = grid.layout();
        assert_eq!(layout.cells.len(), 12);
        assert!(!layout.up_arrow && !layout.down_arrow);
    }

    #[test]
    fn open_screen_suspends_the_shared_lock_but_not_the_reader() {
        use bevy::prelude::*;
        let mut app = App::new();
        app.init_resource::<InventoryScreen>()
            .init_resource::<crate::logbook::LogbookScreen>()
            .add_systems(Update, publish_lock);
        app.update();
        assert!(
            !app.world()
                .resource::<crate::logbook::LogbookScreen>()
                .suspended()
        );
        app.world_mut()
            .resource_mut::<InventoryScreen>()
            .waiting_for_release = true;
        app.update();
        let logbook = app.world().resource::<crate::logbook::LogbookScreen>();
        assert!(logbook.suspended() && !logbook.reading());
        app.world_mut()
            .resource_mut::<InventoryScreen>()
            .waiting_for_release = false;
        app.update();
        assert!(
            !app.world()
                .resource::<crate::logbook::LogbookScreen>()
                .suspended()
        );
    }

    #[test]
    fn names_break_at_spaces_from_byte_thirteen() {
        assert_eq!(name_lines("BONE KEY"), ["BONE KEY"]);
        assert_eq!(
            name_lines("'ELVEN ARTIFACTS VIII'"),
            ["'ELVEN ARTIFACTS", "VIII'"]
        );
        assert_eq!(name_lines("ABCDEFGHIJKLMNOP "), ["ABCDEFGHIJKLMNOP "]);
    }
}
