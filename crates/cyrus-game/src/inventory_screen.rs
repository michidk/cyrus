//! Inventory screen list, cursor, layout and lifecycle, independent of screen presentation.
//! Rules follow preservation's `docs/engine/hud.md` inventory screen section.
use anyhow::Context;
use cyrus_script::Attribute;

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

#[derive(Default)]
enum CloseWait {
    #[default]
    Inventory,
    Escape,
    Walk,
    Activate,
    Enter,
}

/// Shared inventory-screen lifecycle. Presentation draws [`InventoryScreen::layout`].
/// Opening waits for the inventory key's release; closing keeps input suspended until
/// the ordered Inventory, Escape, Walk, Activate and Enter release waits complete.
#[derive(bevy::prelude::Resource, Default, serde::Serialize)]
pub struct InventoryScreen {
    pub open: bool,
    pub waiting_for_release: bool,
    /// Gameplay keeps the opening key until it is released, before the first update.
    pub opening: bool,
    /// True only for the current application update when a changed BIOS tick admits a draw.
    /// Presentation advances the shared candle once, including the final exit draw.
    #[serde(skip)]
    pub draw_admitted: bool,
    pub layout: Layout,
    pub error: Option<String>,
    #[serde(skip)]
    pub grid: InventoryGrid,
    #[serde(skip)]
    close_wait: CloseWait,
    #[serde(skip)]
    checkpoint: Option<crate::clock::ClockCheckpoint>,
    #[serde(skip)]
    last_tick: Option<u32>,
    #[serde(skip)]
    paused_virtual_time: bool,
    /// Mouse buttons held at the previous update's input poll.
    #[serde(skip)]
    mouse_held: [bool; 2],
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

    // Selection is sampled once, after the first two waits, not at the close request.
    fn poll_close(
        &mut self,
        held: &impl Fn(crate::player::input::Action) -> bool,
        escape: bool,
        enter: bool,
        hud: Option<&crate::script::ScriptHud>,
    ) -> bool {
        use crate::player::input::Action;
        loop {
            if match self.close_wait {
                CloseWait::Inventory => held(Action::Inventory),
                CloseWait::Escape => escape,
                CloseWait::Walk => held(Action::Walk),
                CloseWait::Activate => held(Action::Use),
                CloseWait::Enter => enter,
            } {
                return false;
            }
            self.close_wait = match self.close_wait {
                CloseWait::Inventory => CloseWait::Escape,
                CloseWait::Escape => {
                    if let Some(hud) = hud {
                        hud.set_inventory_candle(false);
                    }
                    if (held(Action::Use) || enter)
                        && let (Some(id), Some(hud)) = (self.grid.selection(), hud)
                        && let Err(error) = hud.select_item(id)
                    {
                        log::warn!("inventory: selecting item {id}: {error:#}");
                    }
                    CloseWait::Walk
                }
                CloseWait::Walk => CloseWait::Activate,
                CloseWait::Activate => CloseWait::Enter,
                CloseWait::Enter => {
                    self.waiting_for_release = false;
                    return true;
                }
            };
        }
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
    inputs: crate::player::input::Inputs,
    time: bevy::prelude::Res<bevy::prelude::Time<bevy::prelude::Real>>,
    mut screen: bevy::prelude::ResMut<InventoryScreen>,
    (hud, actors, logbook, console): (
        Option<bevy::prelude::Res<crate::script::ScriptHud>>,
        Option<bevy::prelude::Res<crate::script::ScriptActors>>,
        Option<bevy::prelude::Res<crate::logbook::LogbookScreen>>,
        Option<bevy::prelude::Res<cyrus_console::Console>>,
    ),
    mut clock: Option<bevy::prelude::ResMut<crate::clock::SimulationClock>>,
    mut virtual_time: Option<bevy::prelude::ResMut<bevy::prelude::Time<bevy::prelude::Virtual>>>,
    mut sounds: Option<bevy::prelude::MessageWriter<cyrus_world::sound::SoundCue>>,
) {
    use bevy::prelude::KeyCode;
    screen.draw_admitted = false;
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
        screen.close_wait = CloseWait::default();
        screen.checkpoint = None;
        screen.last_tick = None;
        return;
    }
    use crate::player::input::{Action, Input, Mouse};
    let devices = inputs.devices();
    let held = |action| inputs.pressed(action);
    let escape = keys.pressed(KeyCode::Escape);
    let enter = keys.pressed(KeyCode::Enter);
    if screen.waiting_for_release {
        if screen.poll_close(&held, escape, enter, hud.as_deref()) {
            resume(&mut screen, &mut virtual_time, &mut clock);
        }
        return;
    }
    if !screen.open {
        let opening = inputs.just_pressed(Action::Inventory);
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
            // `attr_player_type` value 1 empties the list.
            let world = actors.as_ref().and_then(|actors| actors.current.as_ref());
            let player_type = world
                .context("inventory: no world")?
                .player_activation_state()
                .ok()
                .and_then(|(id, _)| {
                    world?
                        .actors()
                        .get(id.index)
                        .map(|a| a.attributes()[Attribute::PlayerType])
                });
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
        screen.opening = held(Action::Inventory);
        screen.mouse_held = Mouse::ALL.map(|mouse| devices.mouse_held(mouse));
        return;
    }
    if screen.last_tick == Some(tick) {
        return;
    }
    screen.last_tick = Some(tick);
    screen.draw_admitted = true;
    // Each update polls input once, so a mouse binding reads the release since the last update.
    let mouse = Mouse::ALL.map(|mouse| devices.mouse_held(mouse));
    let previous = std::mem::replace(&mut screen.mouse_held, mouse);
    let held = |action| {
        inputs.bindings().inputs(action).any(|input| match input {
            Input::Mouse(button) => crate::player::input::polled_release(previous, mouse, button),
            input => devices.pressed(input),
        })
    };
    let directions = Directions {
        left: held(Action::Left),
        right: held(Action::Right),
        up: held(Action::Forward),
        down: held(Action::Back),
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
    if held(Action::Inventory) || held(Action::Use) || held(Action::Walk) || escape || enter {
        screen.open = false;
        screen.waiting_for_release = true;
        screen.close_wait = CloseWait::default();
        if screen.poll_close(&held, escape, enter, hud.as_deref()) {
            resume(&mut screen, &mut virtual_time, &mut clock);
        }
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
    fn close_asset() -> cyrus_assets::hud::HudAsset {
        cyrus_assets::hud::HudAsset {
            labels: Default::default(),
            images: vec![],
            candle_mode: 0,
            candle: [0; 2],
            candle_size: [0; 2],
            pickup: [0; 2],
            prompt: [0; 2],
            override_ticks: 0,
            compass: [0; 2],
            compass_size: [0; 2],
            world_compass: Default::default(),
            powerup_slots: [[0; 2]; 2],
            powerup_sizes: vec![],
            logbook: [0; 2],
            logbook_size: [0; 2],
            walk_mode: false,
            auto_defend: false,
            notices: Default::default(),
            inventory: [1, 2]
                .into_iter()
                .map(|id| {
                    (
                        id,
                        cyrus_assets::hud::InventoryItem {
                            count: 1,
                            total: 1,
                            player_max: 1,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            start_items: vec![],
            items: vec![],
            selected: Some(1),
            bindings: [0; 15],
            font: Default::default(),
            text_alpha: [0; 2],
            player_text_alpha: [0; 2],
            used_text_alpha: [0; 2],
            torch_time: None,
            inventory_screen: Default::default(),
            island_map: Default::default(),
        }
    }

    fn close_app(
        asset: &cyrus_assets::hud::HudAsset,
        bindings: crate::player::input::PlayerBindings,
    ) -> bevy::prelude::App {
        use bevy::prelude::*;
        let mut app = App::new();
        app.insert_resource(State::new(crate::AppState::InGame))
            .insert_resource(Time::<Real>::default())
            .insert_resource(Time::<Virtual>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::logbook::LogbookScreen>()
            .insert_resource(bindings);
        register(&mut app);
        let hud = crate::script::ScriptHud::default();
        hud.configure(asset, &crate::preferences::Preferences::default());
        let (items, selected) = hud.inventory_list().unwrap();
        assert!(items.len() >= 2);
        {
            let mut screen = app.world_mut().resource_mut::<InventoryScreen>();
            screen.open = true;
            screen.grid.open(items, selected);
            assert!(screen.grid.update(RIGHT));
            screen.layout = screen.grid.layout();
            screen.paused_virtual_time = true;
        }
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        hud.set_inventory_candle(true);
        app.insert_resource(hud);
        app
    }

    fn close_tick(app: &mut bevy::prelude::App, held: &[bevy::prelude::KeyCode]) {
        use bevy::prelude::*;
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        for &key in held {
            keys.press(key);
        }
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(60));
        app.update();
    }

    #[test]
    fn admitted_draw_is_one_pulse_per_changed_tick_through_exit() {
        use bevy::prelude::*;
        let mut app = close_app(&close_asset(), close_bindings());
        app.world_mut().resource_mut::<InventoryScreen>().opening = true;
        close_tick(&mut app, &[KeyCode::KeyI]);
        assert!(!app.world().resource::<InventoryScreen>().draw_admitted);
        close_tick(&mut app, &[]);
        assert!(!app.world().resource::<InventoryScreen>().draw_admitted);
        close_tick(&mut app, &[]);
        assert!(app.world().resource::<InventoryScreen>().draw_admitted);
        app.update();
        assert!(!app.world().resource::<InventoryScreen>().draw_admitted);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();
        assert!(app.world().resource::<InventoryScreen>().draw_admitted);
        app.update();
        assert!(!app.world().resource::<InventoryScreen>().draw_admitted);
        close_tick(&mut app, &[KeyCode::Escape]);
        assert!(app.world().resource::<InventoryScreen>().draw_admitted);
        assert!(
            app.world()
                .resource::<InventoryScreen>()
                .waiting_for_release
        );
        close_tick(&mut app, &[KeyCode::Escape]);
        assert!(!app.world().resource::<InventoryScreen>().draw_admitted);
        close_tick(&mut app, &[]);
        assert!(!app.world().resource::<InventoryScreen>().draw_admitted);
        assert!(!app.world().resource::<InventoryScreen>().suspended());
    }

    fn close_bindings() -> crate::player::input::PlayerBindings {
        use crate::player::input::{Action, Input};
        [
            (Action::Inventory, 23),
            (Action::Walk, 42),
            (Action::Use, 29),
        ]
        .into_iter()
        .map(|(action, code)| (action, Input::from_binding(code).unwrap().unwrap()))
        .collect()
    }

    fn close_state(app: &bevy::prelude::App, selected: Option<u16>, waiting: bool, timer: u32) {
        use bevy::prelude::*;
        let screen = app.world().resource::<InventoryScreen>();
        assert!(!screen.open);
        assert_eq!(screen.waiting_for_release, waiting);
        assert_eq!(screen.suspended(), waiting);
        assert_eq!(
            app.world()
                .resource::<crate::logbook::LogbookScreen>()
                .suspended(),
            waiting
        );
        let hud = app.world().resource::<crate::script::ScriptHud>();
        assert_eq!(hud.inventory_list().unwrap().1, selected);
        assert_eq!(hud.health_feedback_ticks(), timer);
        assert_eq!(app.world().resource::<Time<Virtual>>().is_paused(), waiting);
    }

    #[test]
    fn closing_samples_confirmation_after_early_release_waits() {
        use bevy::prelude::KeyCode::*;
        let mut app = close_app(&close_asset(), close_bindings());
        close_tick(&mut app, &[KeyI, Enter]);
        close_state(&app, Some(1), true, 50);
        close_tick(&mut app, &[KeyI]);
        close_tick(&mut app, &[]);
        close_state(&app, Some(1), false, 0);
        let mut app = close_app(&close_asset(), close_bindings());
        close_tick(&mut app, &[Escape]);
        close_tick(&mut app, &[Escape, ControlLeft]);
        close_state(&app, Some(1), true, 50);
        close_tick(&mut app, &[ControlLeft]);
        close_state(&app, Some(2), true, 0);
        close_tick(&mut app, &[]);
        close_state(&app, Some(2), false, 0);
    }

    #[test]
    fn closing_does_not_restart_completed_waits_or_reselect() {
        use bevy::prelude::KeyCode::*;
        let mut app = close_app(&close_asset(), close_bindings());
        close_tick(&mut app, &[KeyI, Escape, Enter]);
        close_state(&app, Some(1), true, 50);
        close_tick(&mut app, &[Escape, Enter]);
        close_tick(&mut app, &[KeyI, ShiftLeft, ControlLeft]);
        close_state(&app, Some(2), true, 0);
        // Inventory and Escape waits are complete. Walk release advances to Activate.
        close_tick(&mut app, &[KeyI, Escape, ControlLeft]);
        // Walk is pressed again after its wait. Activate release advances to Enter.
        close_tick(&mut app, &[KeyI, Escape, ShiftLeft, Enter]);
        close_state(&app, Some(2), true, 0);
        // Even a newly re-pressed Activate cannot restart a completed wait.
        close_tick(&mut app, &[KeyI, Escape, ShiftLeft, ControlLeft]);
        close_state(&app, Some(2), false, 0);
        assert_eq!(
            app.world()
                .resource::<crate::script::ScriptHud>()
                .inventory_list()
                .unwrap()
                .0,
            vec![(1, 1), (2, 1)]
        );
    }

    #[test]
    fn closing_empty_cursor_and_state_departure_do_not_select() {
        use bevy::prelude::*;
        let mut app = close_app(&close_asset(), close_bindings());
        app.world_mut()
            .resource_mut::<InventoryScreen>()
            .grid
            .open(vec![], None);
        close_tick(&mut app, &[KeyCode::Enter]);
        close_state(&app, Some(1), true, 0);
        close_tick(&mut app, &[]);
        close_state(&app, Some(1), false, 0);
        let mut app = close_app(&close_asset(), close_bindings());
        close_tick(&mut app, &[KeyCode::Escape, KeyCode::Enter]);
        app.insert_resource(State::new(crate::AppState::Menu));
        close_tick(&mut app, &[KeyCode::Enter]);
        assert!(!app.world().resource::<InventoryScreen>().suspended());
        assert_eq!(
            app.world()
                .resource::<crate::script::ScriptHud>()
                .inventory_list()
                .unwrap()
                .1,
            Some(1)
        );
    }
}
