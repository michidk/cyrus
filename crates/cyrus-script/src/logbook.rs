//! Explicit logbook text layout and scrolling, independent of screen presentation.
use anyhow::{Context, Result};
use cyrus_world::heading::wrap;

/// One laid-out line, in original 640×480 screen coordinates before scrolling.
#[derive(Debug, PartialEq, Eq)]
pub struct LogbookLine {
    pub text: String,
    /// Optional authored style byte before a comma. Font selection belongs to presentation.
    pub style: Option<u8>,
    pub y: u32,
}

/// Inputs for one reading-screen update, after the host resolves installed bindings.
#[derive(Default)]
pub struct LogbookInput {
    pub home: bool,
    pub end: bool,
    pub up: bool,
    pub down: bool,
    pub page_up: bool,
    pub page_down: bool,
    pub close: bool,
    /// Installed A modifier selects the same large step as either Page key.
    pub fast: bool,
}

/// Explicit reading session built from accepted logbook entries, never install writes.
/// Rules follow preservation's `docs/engine/hud.md` logbook reading section.
/// This core does not open a game screen, resolve keys, pause gameplay or draw artwork.
pub struct Logbook {
    lines: Vec<LogbookLine>,
    offset: u32,
    maximum: u32,
    open: bool,
    rotation: i32,
}

impl Logbook {
    /// Lay out already separated entries and start at the newest text.
    /// Entries must contain one physical line each; CR/LF and NUL require storage decoding.
    /// The optional style prefix is one ASCII byte followed by a comma.
    pub fn new(entries: &[String]) -> Result<Self> {
        let mut lines = Vec::new();
        let mut height = 0u32;
        for (entry, text) in entries.iter().enumerate() {
            anyhow::ensure!(
                !text.contains(['\r', '\n', '\0']),
                "logbook entry {entry}: expected one decoded physical line"
            );
            let (style, text) =
                if text.as_bytes().get(1) == Some(&b',') && text.as_bytes()[0].is_ascii() {
                    (Some(text.as_bytes()[0]), &text[2..])
                } else {
                    (None, text.as_str())
                };
            let mut start = 0;
            let mut search = 65;
            while search < text.len().saturating_sub(1) {
                let Some(relative) = text.as_bytes()[search..text.len() - 1]
                    .iter()
                    .position(|&byte| byte == b' ')
                else {
                    break;
                };
                let end = search + relative;
                push_line(&mut lines, &text[start..end], style, &mut height, entry)?;
                start = end + 1;
                search = end.saturating_add(66);
            }
            push_line(&mut lines, &text[start..], style, &mut height, entry)?;
            height = height.checked_add(8).with_context(|| {
                format!("logbook entry {entry}: layout height exceeds screen coordinate range")
            })?;
        }
        let maximum = height.saturating_sub(340);
        Ok(Self {
            lines,
            offset: maximum,
            maximum,
            open: true,
            rotation: 0,
        })
    }

    /// Laid-out lines in entry order, including lines outside the current viewport.
    pub fn lines(&self) -> &[LogbookLine] {
        &self.lines
    }

    /// Third-axis scroll angle, in a 2048-unit revolution.
    pub fn rotation(&self) -> i32 {
        self.rotation
    }

    /// Subtract this offset from each line's y coordinate when presenting it.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Maximum scroll offset in original screen pixels, zero for short books.
    pub fn maximum(&self) -> u32 {
        self.maximum
    }

    /// Whether this explicit reading session still accepts navigation input.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Apply Home, End, up, down and close in original order. Closed sessions ignore input.
    /// Page input selects a shared 50-pixel step; ordinary scrolling uses 10 pixels.
    pub fn update(&mut self, input: LogbookInput) {
        if !self.open {
            return;
        }
        if input.home {
            self.offset = 0;
        }
        if input.end {
            self.offset = self.maximum;
        }
        let fast = input.fast || input.page_up || input.page_down;
        let step = if fast { 50 } else { 10 };
        if (input.up || input.page_up) && self.offset > 0 {
            self.offset = self.offset.saturating_sub(step);
            self.rotation = wrap(self.rotation - if fast { 100 } else { 20 });
        }
        if (input.down || input.page_down) && self.offset < self.maximum {
            self.offset = self.offset.saturating_add(step).min(self.maximum);
            self.rotation = wrap(self.rotation + if fast { 100 } else { 20 });
        }
        if input.close {
            self.open = false;
        }
    }
}

fn push_line(
    lines: &mut Vec<LogbookLine>,
    text: &str,
    style: Option<u8>,
    height: &mut u32,
    entry: usize,
) -> Result<()> {
    let y = height.checked_add(70).with_context(|| {
        format!("logbook entry {entry}: line position exceeds screen coordinate range")
    })?;
    *height = height.checked_add(12).with_context(|| {
        format!("logbook entry {entry}: layout height exceeds screen coordinate range")
    })?;
    lines.push(LogbookLine {
        text: text.to_owned(),
        style,
        y,
    });
    Ok(())
}

/// Shared reading-screen lifecycle. Presentation supplies validated artwork readiness.
/// Closing retains the input lock until all exit controls have been released.
#[derive(bevy::prelude::Resource, Default, serde::Serialize)]
pub struct LogbookScreen {
    pub ready: bool,
    pub open: bool,
    pub waiting_for_release: bool,
    pub offset: u32,
    pub maximum: u32,
    pub rotation: i32,
    pub error: Option<String>,
    #[serde(skip)]
    pub book: Option<Logbook>,
    #[serde(skip)]
    manual_entry: Option<String>,
    #[serde(skip)]
    manual_cursor_visible: bool,
    #[serde(skip)]
    manual_waiting_for_release: bool,
    /// Set by the inventory screen driver: the inventory screen holds the same input, world and
    /// clock suspension as the reader, so every `suspended()` caller covers both.
    #[serde(skip)]
    pub inventory_open: bool,
    /// Set by the island map screen driver, which holds the same suspension.
    #[serde(skip)]
    pub map_open: bool,
    #[serde(skip)]
    checkpoint: Option<crate::clock::ClockCheckpoint>,
    #[serde(skip)]
    last_tick: Option<u32>,
    #[serde(skip)]
    log_released: bool,
    #[serde(skip)]
    paused_virtual_time: bool,
}
impl LogbookScreen {
    /// Manual dialog bytes, including authored CR/VT, while it owns keyboard focus.
    /// Presentation reads this state; game input and journal storage own all edits.
    pub fn manual_entry(&self) -> Option<&str> {
        self.manual_entry.as_deref()
    }

    /// Cursor phase for the manual dialog. It starts hidden and toggles only on a
    /// changed sampled BIOS tick divisible by six, without catching up missed ticks.
    pub fn manual_entry_cursor_visible(&self) -> bool {
        self.manual_entry.is_some() && self.manual_cursor_visible
    }

    /// Includes exit-key release, during which gameplay still must not consume input.
    pub fn suspended(&self) -> bool {
        self.reading() || self.inventory_open || self.map_open
    }
    /// The reader alone, which hides the HUD; the inventory screen keeps the frozen frame visible.
    pub fn reading(&self) -> bool {
        self.open || self.waiting_for_release
    }
}

pub(crate) fn register(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<LogbookScreen>()
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .add_systems(
            PreUpdate,
            drive
                .after(cyrus_console::ConsoleInput)
                .after(bevy::input::InputSystems),
        );
}

#[allow(clippy::too_many_arguments)] // Owns one shared input and clock transition.
fn drive(
    state: bevy::prelude::Res<bevy::prelude::State<cyrus_game::AppState>>,
    keys: bevy::prelude::Res<bevy::prelude::ButtonInput<bevy::prelude::KeyCode>>,
    inputs: crate::player::input::Inputs,
    time: bevy::prelude::Res<bevy::prelude::Time<bevy::prelude::Real>>,
    mut keyboard: bevy::prelude::MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut screen: bevy::prelude::ResMut<LogbookScreen>,
    hud: Option<bevy::prelude::Res<crate::script::ScriptHud>>,
    console: Option<bevy::prelude::Res<cyrus_console::Console>>,
    mut clock: Option<bevy::prelude::ResMut<crate::clock::SimulationClock>>,
    mut virtual_time: Option<bevy::prelude::ResMut<bevy::prelude::Time<bevy::prelude::Virtual>>>,
) {
    use bevy::prelude::KeyCode;
    let tick = crate::clock::bios_tick(time.elapsed());
    let events: Vec<_> = keyboard.read().cloned().collect();
    if *state.get() != cyrus_game::AppState::InGame {
        if screen.paused_virtual_time {
            if let Some(time) = virtual_time.as_mut() {
                time.unpause();
            }
            screen.paused_virtual_time = false;
        }
        screen.open = false;
        screen.waiting_for_release = false;
        screen.book = None;
        screen.manual_entry = None;
        screen.manual_waiting_for_release = false;
        screen.checkpoint = None;
        screen.last_tick = None;
        return;
    }
    use crate::player::input::Action;
    let held = |action| inputs.pressed(action);
    let closing = held(Action::Log) || held(Action::Use) || keys.pressed(KeyCode::Escape);
    if screen.waiting_for_release {
        if !closing {
            screen.waiting_for_release = false;
            if screen.paused_virtual_time {
                if let Some(time) = virtual_time.as_mut() {
                    time.unpause();
                }
                screen.paused_virtual_time = false;
            }
            if let (Some(clock), Some(checkpoint)) = (clock.as_mut(), screen.checkpoint.take()) {
                clock.restore(checkpoint, tick);
            }
        }
        return;
    }
    if !screen.open {
        let opening = inputs.just_pressed(Action::Log);
        if !opening || !screen.ready || screen.inventory_open || console.is_some_and(|c| c.open) {
            return;
        }
        let result = hud
            .as_ref()
            .context("logbook: HUD state unavailable")
            .and_then(|hud| reader_book(hud));
        match result {
            Ok(book) => {
                screen.error = None;
                if let Some(time) = virtual_time.as_mut()
                    && !time.is_paused()
                {
                    time.pause();
                    screen.paused_virtual_time = true;
                }
                if let Some(hud) = hud.as_ref() {
                    let _ = hud.presentation();
                }
                screen.offset = book.offset();
                screen.maximum = book.maximum();
                screen.book = Some(book);
                screen.open = true;
                screen.rotation = 0;
                screen.last_tick = Some(tick);
                screen.log_released = false;
                screen.checkpoint = clock.as_ref().map(|clock| clock.checkpoint());
            }
            Err(error) => {
                log::warn!("logbook: {error:#}");
                screen.error = Some(format!("{error:#}"));
            }
        }
        return;
    }
    if screen.manual_waiting_for_release {
        if !keys.any_pressed([KeyCode::Enter, KeyCode::Escape]) {
            screen.manual_waiting_for_release = false;
        }
        return;
    }
    if let Some(mut text) = screen.manual_entry.take() {
        if screen.last_tick != Some(tick) {
            screen.last_tick = Some(tick);
            if tick.is_multiple_of(6) {
                screen.manual_cursor_visible ^= true;
            }
        }
        let mut finish = keys.pressed(KeyCode::Escape).then_some(false);
        // Modifier messages preserve event order, including a toggle within one host frame.
        let mut shifts = [
            keys.pressed(KeyCode::ShiftLeft),
            keys.pressed(KeyCode::ShiftRight),
        ];
        for event in events.iter().rev().filter(|event| !event.repeat) {
            if let Some(index) = shift_index(event.key_code) {
                shifts[index] = event.state == bevy::input::ButtonState::Released;
            }
        }
        for event in &events {
            if finish.is_some() {
                break;
            }
            if let Some(index) = shift_index(event.key_code) {
                shifts[index] = event.state == bevy::input::ButtonState::Pressed;
                continue;
            }
            if event.state != bevy::input::ButtonState::Pressed {
                continue;
            }
            match edit_manual(&mut text, event, shifts.contains(&true)) {
                ManualResult::Continue => {}
                ManualResult::Accept => {
                    finish = Some(true);
                    break;
                }
                ManualResult::Cancel => {
                    finish = Some(false);
                    break;
                }
            }
        }
        match finish {
            Some(true) => {
                let result = (|| {
                    let hud = hud.as_ref().context("logbook: HUD state unavailable")?;
                    let mut entries = hud.entries()?;
                    entries.push(format!("9,{text}"));
                    let book = reader_entries(&entries)?;
                    hud.append_manual_entry(&text)?;
                    Ok::<_, anyhow::Error>(book)
                })();
                match result {
                    Ok(book) => {
                        screen.offset = book.offset();
                        screen.maximum = book.maximum();
                        screen.rotation = 0;
                        screen.book = Some(book);
                        screen.error = None;
                        screen.manual_waiting_for_release = true;
                    }
                    Err(error) => {
                        screen.error = Some(format!("logbook: {error:#}"));
                        screen.manual_entry = Some(text);
                    }
                }
            }
            Some(false) => {
                screen.manual_waiting_for_release = true;
            }
            None => {
                screen.manual_entry = Some(text);
            }
        }
        return;
    }
    if inputs.just_pressed(Action::View) {
        screen.manual_entry = Some(String::new());
        screen.manual_cursor_visible = false;
        screen.manual_waiting_for_release = keys.any_pressed([KeyCode::Enter, KeyCode::Escape]);
        screen.last_tick = Some(tick);
        screen.error = None;
        return;
    }
    if !held(Action::Log) {
        screen.log_released = true;
    }
    if (held(Action::Log) && screen.log_released)
        || held(Action::Use)
        || keys.pressed(KeyCode::Escape)
    {
        screen.open = false;
        screen.waiting_for_release = true;
        return;
    }
    if screen.last_tick == Some(tick) {
        return;
    }
    screen.last_tick = Some(tick);
    let fast = held(Action::Walk) || keys.any_pressed([KeyCode::PageUp, KeyCode::PageDown]);
    let input = LogbookInput {
        home: keys.pressed(KeyCode::Home),
        end: keys.pressed(KeyCode::End),
        up: held(Action::Forward),
        down: held(Action::Back),
        page_up: keys.pressed(KeyCode::PageUp),
        page_down: keys.pressed(KeyCode::PageDown),
        ..Default::default()
    };
    if let Some(book) = screen.book.as_mut() {
        book.update(LogbookInput { fast, ..input });
        let (offset, rotation) = (book.offset(), book.rotation());
        screen.offset = offset;
        screen.rotation = rotation;
    }
}

fn reader_book(hud: &crate::script::ScriptHud) -> Result<Logbook> {
    reader_entries(&hud.entries()?)
}

fn reader_entries(entries: &[String]) -> Result<Logbook> {
    let lines: Vec<_> = entries
        .iter()
        .flat_map(|entry| entry.split(['\r', '\n', '\0']))
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    let book = Logbook::new(&lines)?;
    for line in book.lines() {
        anyhow::ensure!(line.text.is_ascii(), "logbook: unsupported non-ASCII text");
        anyhow::ensure!(
            matches!(line.style, None | Some(b'0'..=b'9')),
            "logbook: unsupported font style {:?}",
            line.style
        );
    }
    Ok(book)
}

fn shift_index(key: bevy::prelude::KeyCode) -> Option<usize> {
    match key {
        bevy::prelude::KeyCode::ShiftLeft => Some(0),
        bevy::prelude::KeyCode::ShiftRight => Some(1),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ManualResult {
    Continue,
    Accept,
    Cancel,
}

fn edit_manual(
    text: &mut String,
    event: &bevy::input::keyboard::KeyboardInput,
    shift: bool,
) -> ManualResult {
    use bevy::prelude::KeyCode;
    match event.key_code {
        KeyCode::Escape => return ManualResult::Cancel,
        KeyCode::Backspace if !shift => {
            text.pop();
        }
        KeyCode::Enter if !shift => {
            if !text.is_empty() {
                return ManualResult::Accept;
            }
        }
        KeyCode::Tab => {}
        _ => {
            let control = match event.key_code {
                KeyCode::Backspace => Some("\r"),
                KeyCode::Enter => Some("\x0b"),
                _ => None,
            };
            let logical_text = match &event.logical_key {
                bevy::input::keyboard::Key::Character(text) => Some(text.as_str()),
                bevy::input::keyboard::Key::Space => Some(" "),
                _ => None,
            };
            if let Some(input) = control.or(event.text.as_deref()).or(logical_text) {
                for byte in input
                    .bytes()
                    .filter(|b| b.is_ascii_graphic() || matches!(b, b' ' | b'\r' | 11))
                {
                    if text.len() == 64 {
                        break;
                    }
                    text.push(char::from(byte.to_ascii_uppercase()));
                }
            }
        }
    }
    ManualResult::Continue
}

#[cfg(test)]
mod screen_tests {
    use super::*;
    use crate::player::input::{Action, Input, PlayerBindings};

    fn key(key: KeyCode) -> Input {
        Input::try_from(key).unwrap()
    }
    use bevy::prelude::*;

    fn event(key_code: KeyCode, text: Option<&str>) -> bevy::input::keyboard::KeyboardInput {
        bevy::input::keyboard::KeyboardInput {
            key_code,
            logical_key: bevy::input::keyboard::Key::Character("".into()),
            state: bevy::input::ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    #[test]
    fn manual_controls_preserve_original_bytes_and_cap() {
        let mut text = String::new();
        assert_eq!(
            edit_manual(&mut text, &event(KeyCode::Enter, None), false),
            ManualResult::Continue
        );
        edit_manual(&mut text, &event(KeyCode::KeyA, Some("aBc")), false);
        edit_manual(&mut text, &event(KeyCode::Backspace, None), false);
        edit_manual(&mut text, &event(KeyCode::Backspace, None), true);
        edit_manual(&mut text, &event(KeyCode::Enter, None), true);
        edit_manual(&mut text, &event(KeyCode::Tab, Some("\t")), false);
        assert_eq!(text.as_bytes(), b"AB\r\x0b");
        assert_eq!(
            edit_manual(&mut text, &event(KeyCode::Enter, None), false),
            ManualResult::Accept
        );
        assert_eq!(
            edit_manual(&mut text, &event(KeyCode::Escape, None), true),
            ManualResult::Cancel
        );
        text = "A".repeat(63);
        edit_manual(&mut text, &event(KeyCode::Enter, None), true);
        edit_manual(&mut text, &event(KeyCode::KeyB, Some("b")), false);
        assert_eq!(text.as_bytes(), [b"A".repeat(63), vec![11]].concat());
        edit_manual(&mut text, &event(KeyCode::Backspace, None), false);
        edit_manual(&mut text, &event(KeyCode::KeyB, Some("b")), true);
        assert_eq!(text, format!("{}B", "A".repeat(63)));
    }

    #[test]
    fn manual_dialog_owns_input_and_samples_cursor_without_catchup() {
        let mut app = App::new();
        app.insert_resource(State::new(cyrus_game::AppState::InGame))
            .insert_resource(Time::<Real>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .init_resource::<cyrus_game::preferences::Preferences>()
            .add_systems(Update, drive);
        app.insert_resource(
            [(Action::View, key(KeyCode::AltLeft))]
                .into_iter()
                .collect::<PlayerBindings>(),
        );
        app.insert_resource(LogbookScreen {
            open: true,
            ready: true,
            book: Some(Logbook::new(&["7,Existing".into()]).unwrap()),
            ..Default::default()
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::AltLeft);
        app.update();
        let screen = app.world().resource::<LogbookScreen>();
        assert_eq!(screen.manual_entry(), Some(""));
        assert!(!screen.manual_entry_cursor_visible());
        assert!(screen.suspended());
        app.world_mut()
            .write_message(event(KeyCode::KeyA, Some("a")));
        app.update();
        assert_eq!(
            app.world().resource::<LogbookScreen>().manual_entry(),
            Some("A")
        );
        // Tick six is sampled once; repeated host frames at that tick do not toggle.
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(330));
        app.update();
        assert!(
            app.world()
                .resource::<LogbookScreen>()
                .manual_entry_cursor_visible()
        );
        app.update();
        assert!(
            app.world()
                .resource::<LogbookScreen>()
                .manual_entry_cursor_visible()
        );
        // Skip tick twelve and sample thirteen. No catchup toggle is allowed.
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(390));
        app.update();
        assert!(
            app.world()
                .resource::<LogbookScreen>()
                .manual_entry_cursor_visible()
        );
        app.world_mut().write_message(event(KeyCode::Escape, None));
        app.update();
        let screen = app.world().resource::<LogbookScreen>();
        assert_eq!(screen.manual_entry(), None);
        assert!(screen.open && screen.suspended());
        assert_eq!(screen.book.as_ref().unwrap().lines()[0].text, "Existing");
    }

    #[test]
    fn manual_entry_drains_held_return_before_editing() {
        let mut app = App::new();
        app.insert_resource(State::new(cyrus_game::AppState::InGame))
            .insert_resource(Time::<Real>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .init_resource::<cyrus_game::preferences::Preferences>()
            .add_systems(Update, drive);
        app.insert_resource(
            [(Action::View, key(KeyCode::AltLeft))]
                .into_iter()
                .collect::<PlayerBindings>(),
        );
        app.insert_resource(LogbookScreen {
            open: true,
            ready: true,
            book: Some(Logbook::new(&["7,Existing".into()]).unwrap()),
            ..Default::default()
        });
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::AltLeft);
            keys.press(KeyCode::Enter);
        }
        app.update();
        app.world_mut()
            .write_message(event(KeyCode::KeyA, Some("a")));
        app.update();
        assert_eq!(
            app.world().resource::<LogbookScreen>().manual_entry(),
            Some("")
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Enter);
        app.update();
        app.world_mut()
            .write_message(event(KeyCode::KeyB, Some("b")));
        app.update();
        assert_eq!(
            app.world().resource::<LogbookScreen>().manual_entry(),
            Some("B")
        );
        assert!(app.world().resource::<LogbookScreen>().suspended());
    }

    #[test]
    fn screen_retains_exit_lock_and_restores_clock_after_release() {
        let mut app = App::new();
        app.insert_resource(State::new(cyrus_game::AppState::InGame))
            .insert_resource(Time::<Real>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .init_resource::<cyrus_game::preferences::Preferences>()
            .add_systems(Update, drive);
        let mut clock = crate::clock::SimulationClock::new(cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 1,
            max_frame_rate: 36,
            use_smooth_fps: false,
            use_smooth_divisor: false,
        })
        .unwrap();
        clock.advance(0);
        let checkpoint = clock.checkpoint();
        let mut expected = clock.clone();
        app.insert_resource(clock);
        app.insert_resource(
            [
                (Action::Log, key(KeyCode::KeyL)),
                (Action::Use, key(KeyCode::Space)),
            ]
            .into_iter()
            .collect::<PlayerBindings>(),
        );
        app.insert_resource(LogbookScreen {
            open: true,
            ready: true,
            book: Some(Logbook::new(&vec!["entry".into(); 20]).unwrap()),
            checkpoint: Some(checkpoint),
            ..Default::default()
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyL);
        app.update();
        assert!(
            app.world().resource::<LogbookScreen>().open,
            "opening log press must not close the screen"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyL);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyL);
        app.update();
        let screen = app.world().resource::<LogbookScreen>();
        assert!(!screen.open && screen.suspended());
        app.update();
        assert!(app.world().resource::<LogbookScreen>().suspended());
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(10));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyL);
        app.update();
        assert!(!app.world().resource::<LogbookScreen>().suspended());
        let tick = crate::clock::bios_tick(std::time::Duration::from_secs(10));
        expected.rebase(tick);
        assert_eq!(
            app.world_mut()
                .resource_mut::<crate::clock::SimulationClock>()
                .advance(tick + 1),
            expected.advance(tick + 1)
        );
    }
}
