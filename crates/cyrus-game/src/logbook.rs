//! Explicit logbook text layout and scrolling, independent of screen presentation.
use anyhow::{Context, Result};

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
            self.rotation = (self.rotation - if fast { 100 } else { 20 }) & 2047;
        }
        if (input.down || input.page_down) && self.offset < self.maximum {
            self.offset = self.offset.saturating_add(step).min(self.maximum);
            self.rotation = (self.rotation + if fast { 100 } else { 20 }) & 2047;
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
    pub bindings: [u8; 15],
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
    /// Includes exit-key release, during which gameplay still must not consume input.
    pub fn suspended(&self) -> bool {
        self.open || self.waiting_for_release
    }
}

pub(crate) fn register(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<LogbookScreen>().add_systems(
        PreUpdate,
        drive
            .after(cyrus_console::ConsoleInput)
            .after(bevy::input::InputSystems),
    );
}

#[allow(clippy::too_many_arguments)] // Owns one shared input and clock transition.
fn drive(
    state: bevy::prelude::Res<bevy::prelude::State<crate::AppState>>,
    keys: bevy::prelude::Res<bevy::prelude::ButtonInput<bevy::prelude::KeyCode>>,
    time: bevy::prelude::Res<bevy::prelude::Time<bevy::prelude::Real>>,
    mut screen: bevy::prelude::ResMut<LogbookScreen>,
    hud: Option<bevy::prelude::Res<crate::script::ScriptHud>>,
    preferences: bevy::prelude::Res<crate::preferences::Preferences>,
    console: Option<bevy::prelude::Res<cyrus_console::Console>>,
    mut clock: Option<bevy::prelude::ResMut<crate::clock::SimulationClock>>,
    mut virtual_time: Option<bevy::prelude::ResMut<bevy::prelude::Time<bevy::prelude::Virtual>>>,
) {
    use bevy::prelude::KeyCode;
    let tick = crate::clock::bios_tick(time.elapsed());
    if *state.get() != crate::AppState::InGame {
        if screen.paused_virtual_time {
            if let Some(time) = virtual_time.as_mut() {
                time.unpause();
            }
            screen.paused_virtual_time = false;
        }
        screen.open = false;
        screen.waiting_for_release = false;
        screen.book = None;
        screen.checkpoint = None;
        screen.last_tick = None;
        return;
    }
    let bindings = preferences
        .menu()
        .as_ref()
        .ok()
        .and_then(|p| p.as_ref())
        .map_or(screen.bindings, |p| p.bindings);
    let held = |index: usize| {
        crate::player::input::scan_key(bindings[index]).is_some_and(|key| keys.pressed(key))
    };
    let closing = held(12) || held(7) || keys.pressed(KeyCode::Escape);
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
        let opening =
            crate::player::input::scan_key(bindings[12]).is_some_and(|key| keys.just_pressed(key));
        if !opening || !screen.ready || console.is_some_and(|c| c.open) {
            return;
        }
        let result = (|| {
            let entries = hud
                .as_ref()
                .context("logbook: HUD state unavailable")?
                .entries()?;
            // The original reader skips runs of CR/LF/NUL between physical records.
            let lines: Vec<_> = entries
                .iter()
                .flat_map(|entry| entry.split(['\r', '\n', '\0']))
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect();
            let book = Logbook::new(&lines)?;
            for line in book.lines() {
                anyhow::ensure!(line.text.is_ascii(), "logbook: unsupported non-ASCII text");
                // Styles 1, 2 (gui_low_font) and 5 are not decoded; the rest follow the reader style table.
                anyhow::ensure!(
                    matches!(
                        line.style,
                        None | Some(b'0' | b'3' | b'4' | b'6' | b'7' | b'8' | b'9')
                    ),
                    "logbook: unsupported font style {:?}",
                    line.style
                );
            }
            Ok::<_, anyhow::Error>(book)
        })();
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
    if !held(12) {
        screen.log_released = true;
    }
    if (held(12) && screen.log_released) || held(7) || keys.pressed(KeyCode::Escape) {
        screen.open = false;
        screen.waiting_for_release = true;
        return;
    }
    if screen.last_tick == Some(tick) {
        return;
    }
    screen.last_tick = Some(tick);
    let fast = held(4) || keys.any_pressed([KeyCode::PageUp, KeyCode::PageDown]);
    let input = LogbookInput {
        home: keys.pressed(KeyCode::Home),
        end: keys.pressed(KeyCode::End),
        up: held(0),
        down: held(1),
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

#[cfg(test)]
mod screen_tests {
    use super::*;
    use bevy::prelude::*;

    #[test]
    fn screen_retains_exit_lock_and_restores_clock_after_release() {
        let mut app = App::new();
        app.insert_resource(State::new(crate::AppState::InGame))
            .insert_resource(Time::<Real>::default())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::preferences::Preferences>()
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
        let mut bindings = [0; 15];
        bindings[12] = 38;
        bindings[7] = 57;
        app.insert_resource(LogbookScreen {
            open: true,
            ready: true,
            book: Some(Logbook::new(&vec!["entry".into(); 20]).unwrap()),
            checkpoint: Some(checkpoint),
            bindings,
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
