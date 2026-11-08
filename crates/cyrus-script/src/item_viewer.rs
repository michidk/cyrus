//! Item viewer rules: the pose, fade and exit of the modal that `ShowItem` opens, and the
//! description line sequence. Independent of presentation and audio; the host feeds one update
//! per BIOS-timer tick. Rules follow preservation's `docs/engine/SOUP.md` "Item viewer".
use anyhow::{Context, Result, ensure};

/// Held actions at one update, after the host resolves the installed bindings.
#[derive(Clone, Copy, Default)]
pub struct ViewerInput {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    /// `key_a`: pans.
    pub walk: bool,
    /// `key_b`: depth and roll.
    pub jump: bool,
    /// `key_c`, `key_d` or a mouse button: any of them requests the exit.
    pub exit: bool,
}

/// Where the model is drawn: rotation about the vertical axis, then the horizontal axis, then
/// the view axis (2048 units per turn), and the position in the original's map units.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct ViewerPose {
    pub turn: u16,
    pub pitch: u16,
    pub roll: u16,
    pub position: [f32; 3],
}

/// Distance the entry fade starts the model from its resting place along the view axis.
const ENTRY_DEPTH: f32 = 1490.0;
/// Fade factor step per update.
const FADE_STEP: f64 = 0.15;
const ANGLE_STEP: i32 = 80;
const PAN_STEP: f32 = 40.0;
const DEPTH_STEP: f32 = 80.0;

/// One open viewer. Create it with the item model's three bounding extents.
#[derive(Debug)]
pub struct ItemViewer {
    /// Largest bounding extent.
    largest: i32,
    depth_min: f32,
    depth_max: f32,
    aspect: f32,
    angles: [u16; 3],
    pan: [f32; 2],
    depth: f32,
    fade: f32,
    exiting: bool,
    released: bool,
    exit_angles: [i32; 3],
    /// Offset from the resting position at fade factor one, as it enters.
    entry_offset: [f32; 3],
    /// The place the view flies back to when it closes.
    exit_target: [f32; 3],
    exit_offset: [f32; 3],
}

impl ItemViewer {
    /// The scripted entry (`ShowItem`). `extents` are the model's x, y and z bounding extents,
    /// `aspect` is screen height over width. The model flies in from `(first, second, 1490)`
    /// and flies back to `(second, first, 1490)`: the original's exit swaps the two words.
    pub fn open(extents: [i32; 3], words: [i32; 2], aspect: f32) -> Self {
        let [first, second] = words.map(|word| word as f32);
        Self::new(
            extents,
            aspect,
            [first, second, ENTRY_DEPTH],
            [second, first, ENTRY_DEPTH],
        )
    }

    /// The inventory screen's entry: the model flies in from the cursor cell at visible `row` and
    /// `column` and flies back to it.
    pub fn open_inventory(extents: [i32; 3], row: i32, column: i32, aspect: f32) -> Self {
        let cell = [
            (column * 0x184 - 0x246) as f32,
            (row * 0x17a - 0x1b0) as f32,
        ];
        let start = extents[2] as f32 + 180.0;
        Self::new(
            extents,
            aspect,
            [cell[0], cell[1], ENTRY_DEPTH - start],
            [cell[0], cell[1], ENTRY_DEPTH],
        )
    }

    fn new(extents: [i32; 3], aspect: f32, entry_offset: [f32; 3], exit_target: [f32; 3]) -> Self {
        let largest = extents.into_iter().max().unwrap_or(0);
        let depth_min = extents[2] as f32;
        let start = depth_min + 180.0;
        Self {
            largest,
            depth_min,
            depth_max: (8 * largest) as f32 + start,
            aspect,
            angles: [0; 3],
            pan: [0.0; 2],
            depth: start,
            fade: 1.0,
            exiting: false,
            released: false,
            exit_angles: [0; 3],
            entry_offset,
            exit_target,
            exit_offset: [0.0; 3],
        }
    }

    fn scale(&self) -> f32 {
        let range = self.depth_max - self.depth_min;
        ((self.depth - self.depth_min) as f64 / range as f64 + 0.1) as f32
    }

    /// Pan limits for the current scale: horizontal and vertical half-ranges.
    fn pan_limits(&self) -> [f32; 2] {
        let half = (2 * self.largest) as f32 * self.scale();
        [
            half.round_ties_even(),
            (half * self.aspect).round_ties_even(),
        ]
    }

    /// One update. Returns true once the viewer has finished and must close.
    pub fn update(&mut self, input: ViewerInput) -> bool {
        let moving =
            input.left || input.right || input.up || input.down || input.walk || input.jump;
        if moving {
            let scale = self.scale();
            let step = |positive: bool, negative: bool| i32::from(positive) - i32::from(negative);
            let turn = |angle: u16, delta: i32| (i32::from(angle) + delta * ANGLE_STEP) as u16;
            if input.walk {
                self.pan[0] += step(input.right, input.left) as f32 * PAN_STEP * scale;
                self.pan[1] += step(input.down, input.up) as f32 * PAN_STEP * scale;
            } else if input.jump {
                self.depth += step(input.up, input.down) as f32 * DEPTH_STEP * scale;
                self.angles[2] = turn(self.angles[2], step(input.right, input.left));
            } else {
                self.angles[0] = turn(self.angles[0], step(input.left, input.right));
                self.angles[1] = turn(self.angles[1], step(input.down, input.up));
            }
            self.depth = self.depth.clamp(self.depth_min, self.depth_max);
            let limits = self.pan_limits();
            self.pan[0] = self.pan[0].clamp(-limits[0], limits[0]);
            self.pan[1] = self.pan[1].clamp(-limits[1], limits[1]);
        }
        self.angles = self.angles.map(|angle| angle & 0x7ff);
        if self.fade != 0.0 {
            if self.exiting {
                self.fade = (f64::from(self.fade) + FADE_STEP) as f32;
                if self.fade > 1.0 {
                    return true;
                }
            } else {
                self.fade = ((f64::from(self.fade) - FADE_STEP) as f32).max(0.0);
            }
            self.interpolate_angles();
        }
        if !input.exit || self.exiting {
            self.released = true;
        } else if self.released {
            self.begin_exit();
        }
        false
    }

    fn interpolate_angles(&mut self) {
        let remaining = f64::from(1.0 - self.fade);
        for (angle, from) in self.angles.iter_mut().zip(self.exit_angles) {
            *angle = ((f64::from(from) * remaining).round_ties_even() as i32 & 0x7ff) as u16;
        }
    }

    fn begin_exit(&mut self) {
        self.exiting = true;
        self.exit_angles = self
            .angles
            .map(|angle| i32::from(angle) - if angle > 0x400 { 0x800 } else { 0 });
        self.exit_offset = [
            self.exit_target[0] - self.pan[0],
            self.exit_target[1] - self.pan[1],
            self.exit_target[2] - self.depth,
        ];
        self.fade = (f64::from(self.fade) + FADE_STEP) as f32;
        self.interpolate_angles();
    }

    /// Whether the exit has started (the host stops the description speech then).
    pub fn exiting(&self) -> bool {
        self.exiting
    }

    /// The model pose to draw after the latest update.
    pub fn pose(&self) -> ViewerPose {
        let offset = if self.exiting {
            self.exit_offset
        } else {
            self.entry_offset
        };
        ViewerPose {
            turn: self.angles[0],
            pitch: self.angles[1],
            roll: self.angles[2],
            position: [
                self.pan[0] + offset[0] * self.fade,
                self.pan[1] + offset[1] * self.fade,
                self.depth + offset[2] * self.fade,
            ],
        }
    }
}

/// The four-character RTX label for a formatted tag: longer text is cut to four characters.
fn label(text: &str) -> [u8; 4] {
    let mut label = [0; 4];
    for (slot, byte) in label.iter_mut().zip(text.bytes()) {
        *slot = byte;
    }
    label
}

/// The description lines the viewer plays for one item, in order. The host stops at the first
/// label that has no RTX record, as the original does.
#[derive(Debug)]
pub struct DescriptionLines {
    labels: std::vec::IntoIter<[u8; 4]>,
}

impl DescriptionLines {
    /// `description` is the item's ITEM.INI label; `potion` is the item's `GetPotion` counter.
    pub fn new(item: i32, description: Option<&str>, potion: i32) -> Self {
        let own = description.map(label);
        let numbered = |first: i32, last: i32, format: &dyn Fn(i32) -> String| {
            (first..=last)
                .map(|n| label(&format(n)))
                .collect::<Vec<_>>()
        };
        let labels = match item {
            20 | 22 | 24 | 26 => own
                .into_iter()
                .chain([label(&format!("cip{}", potion.wrapping_add(1)))])
                .collect(),
            36 => vec![label("cibe"), label("cibl")],
            53 => numbered(2, 8, &|n| format!("iij{n}")),
            55 => [label("cikj")]
                .into_iter()
                .chain(numbered(1, 11, &|n| format!("ny{n:02}")))
                .collect(),
            _ => own.into_iter().collect(),
        };
        Self {
            labels: labels.into_iter(),
        }
    }
}

impl Iterator for DescriptionLines {
    type Item = [u8; 4];
    fn next(&mut self) -> Option<Self::Item> {
        self.labels.next()
    }
}

/// What the examine view shows for one item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExamineItem {
    /// Uppercase stem of the item's inventory model; `None` when ITEM.INI names none.
    pub model: Option<String>,
    /// False for a HUD export that predates the inventory model record.
    pub exported: bool,
    pub extents: Option<[i32; 3]>,
    /// The item's `description` RTX label.
    pub description: Option<String>,
}

/// The inventory screen's request to examine its cursor item, at the cursor cell's visible
/// `row` and `column`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct InventoryCell {
    pub item: u16,
    pub row: i32,
    pub column: i32,
}

/// The loaded world's look that the examine view draws with: model lighting inputs in the
/// original's axes and the palette textures resolve through (preservation `engine/hud.md`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct ExamineLook {
    pub ambient: [f32; 3],
    pub directional: [f32; 3],
    /// Direction of the directional light in the original's axes.
    pub direction: [f32; 3],
    /// 256 entries, entry zero black.
    pub palette: Vec<[u8; 3]>,
}

/// Where a description line is in its speech.
#[derive(Default, Debug)]
enum Line {
    #[default]
    Idle,
    /// The audio owner has been asked to start this label and has not answered.
    Requested,
    Playing {
        until: std::time::Duration,
    },
    /// No further lines: the sequence ended or a label has no record.
    Ended,
}

/// The examine view's modal state. The overlay owner sets [`Self::enabled`]; until then pending
/// requests stay with the script world. Gameplay stays suspended while the view is open and until
/// View, Activate, Enter and Escape are released after it closes.
#[derive(bevy::prelude::Resource, Default, serde::Serialize)]
pub struct ExamineScreen {
    /// A presentation exists that draws [`Self::pose`].
    pub enabled: bool,
    pub open: bool,
    pub waiting_for_release: bool,
    /// Uppercase stem of the model being examined.
    pub model: Option<String>,
    /// The pose to draw after the latest update.
    pub pose: Option<ViewerPose>,
    pub error: Option<String>,
    /// The loaded world's look, published by the renderer that owns it.
    pub look: Option<ExamineLook>,
    /// The inventory screen asks to examine its cursor item.
    pub inventory_request: Option<InventoryCell>,
    /// Set when an inventory-entry view closes: true when Activate or Enter was held at the close
    /// request, which closes the inventory screen. The inventory screen takes it.
    pub inventory_result: Option<bool>,
    #[serde(skip)]
    from_inventory: bool,
    #[serde(skip)]
    confirm: bool,
    /// A label the audio owner starts as speech (and its subtitle), then answers through
    /// [`Self::speech_reply`].
    pub speech_request: Option<[u8; 4]>,
    /// The audio owner's answer: the clip length in seconds, or `None` when the label has no
    /// speech record, which ends the line sequence.
    pub speech_reply: Option<Option<f32>>,
    #[serde(skip)]
    viewer: Option<ItemViewer>,
    #[serde(skip)]
    lines: Option<DescriptionLines>,
    #[serde(skip)]
    line: Line,
    #[serde(skip)]
    last_tick: Option<u32>,
    #[serde(skip)]
    checkpoint: Option<crate::clock::ClockCheckpoint>,
    #[serde(skip)]
    paused_virtual_time: bool,
}

impl ExamineScreen {
    /// Gameplay input must not be consumed.
    pub fn suspended(&self) -> bool {
        self.open || self.waiting_for_release
    }

    /// Gameplay and the inventory screen leave the view's request alone until it has run.
    pub fn busy(&self) -> bool {
        self.suspended() || self.inventory_request.is_some() || self.inventory_result.is_some()
    }
}

pub fn register(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.init_resource::<ExamineScreen>().add_systems(
        PreUpdate,
        drive
            .after(cyrus_console::ConsoleInput)
            .after(bevy::input::InputSystems),
    );
}

/// Screen height over width of the 640 by 480 view.
const ASPECT: f32 = 0.75;

#[allow(clippy::too_many_arguments, clippy::type_complexity)] // One shared input, clock and request transition; the tuple only groups optional resources past the system parameter limit.
fn drive(
    state: bevy::prelude::Res<bevy::prelude::State<cyrus_game::AppState>>,
    keys: bevy::prelude::Res<bevy::prelude::ButtonInput<bevy::prelude::KeyCode>>,
    inputs: crate::player::input::Inputs,
    time: bevy::prelude::Res<bevy::prelude::Time<bevy::prelude::Real>>,
    mut screen: bevy::prelude::ResMut<ExamineScreen>,
    (hud, mut actors, console): (
        Option<bevy::prelude::Res<crate::script::ScriptHud>>,
        Option<bevy::prelude::ResMut<crate::script::ScriptActors>>,
        Option<bevy::prelude::Res<cyrus_console::Console>>,
    ),
    mut clock: Option<bevy::prelude::ResMut<crate::clock::SimulationClock>>,
    mut virtual_time: Option<bevy::prelude::ResMut<bevy::prelude::Time<bevy::prelude::Virtual>>>,
    mut sounds: Option<bevy::prelude::MessageWriter<cyrus_world::sound::SoundCue>>,
) {
    use crate::player::input::Action;
    let mut cue = |effect: usize| {
        if let Some(sounds) = sounds.as_mut() {
            sounds.write(cyrus_world::sound::SoundCue::Flat {
                effect,
                volume: 0,
                rate_offset_hz: 0,
            });
        }
    };
    use bevy::prelude::KeyCode;
    let tick = crate::clock::bios_tick(time.elapsed());
    let resume =
        |screen: &mut ExamineScreen,
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
    if *state.get() != cyrus_game::AppState::InGame {
        resume(&mut screen, &mut virtual_time, &mut None);
        *screen = ExamineScreen {
            enabled: screen.enabled,
            ..default_screen()
        };
        return;
    }
    let exit_held = inputs.pressed(Action::View)
        || inputs.pressed(Action::Use)
        || keys.pressed(KeyCode::Enter)
        || keys.pressed(KeyCode::Escape);
    if screen.waiting_for_release {
        if !exit_held {
            screen.waiting_for_release = false;
        }
        return;
    }
    if !screen.open {
        if !screen.enabled || console.is_some_and(|console| console.open) {
            return;
        }
        let Some(hud) = hud else {
            return;
        };
        let cell = screen.inventory_request.take();
        let scripted = if cell.is_none() {
            let Some(world) = actors.as_mut().and_then(|actors| actors.current.as_mut()) else {
                return;
            };
            let Some(request) = world.globals_mut().take_item_viewer_request() else {
                return;
            };
            Some(request)
        } else {
            None
        };
        let (item_id, entry, no_rtx) = match (cell, scripted) {
            (Some(cell), _) => (
                i32::from(cell.item),
                Entry::Inventory {
                    row: cell.row,
                    column: cell.column,
                },
                false,
            ),
            (None, Some(request)) => (
                request.item,
                Entry::Scripted {
                    words: request.words,
                },
                request.no_rtx,
            ),
            (None, None) => return,
        };
        let opened = open(
            &hud,
            actors
                .as_ref()
                .and_then(|actors| actors.current.as_ref())
                .map(|world| world.globals()),
            item_id,
            entry,
        );
        let finish_scripted =
            |actors: &mut Option<bevy::prelude::ResMut<crate::script::ScriptActors>>| {
                if scripted.is_some()
                    && let Some(world) = actors.as_mut().and_then(|actors| actors.current.as_mut())
                {
                    world.globals_mut().finish_item_viewer();
                }
            };
        match opened {
            Ok(Some((viewer, lines, model))) => {
                screen.error = None;
                screen.model = Some(model);
                screen.pose = Some(viewer.pose());
                screen.viewer = Some(viewer);
                screen.lines = (!no_rtx).then_some(lines);
                screen.line = if no_rtx { Line::Ended } else { Line::Idle };
                screen.from_inventory = cell.is_some();
                screen.confirm = false;
                if let Some(time) = virtual_time.as_mut()
                    && !time.is_paused()
                {
                    time.pause();
                    screen.paused_virtual_time = true;
                }
                screen.checkpoint = clock.as_ref().map(|clock| clock.checkpoint());
                screen.last_tick = Some(tick);
                screen.open = true;
                // Sound effect 61 plays when the view opens (preservation engine/hud.md).
                cue(61);
            }
            Ok(None) => {
                finish_scripted(&mut actors);
                screen.inventory_result = cell.map(|_| false);
            }
            Err(error) => {
                log::warn!("item viewer: {error:#}");
                screen.error = Some(format!("{error:#}"));
                finish_scripted(&mut actors);
                screen.inventory_result = cell.map(|_| false);
            }
        }
        return;
    }
    if screen.last_tick == Some(tick) {
        return;
    }
    screen.last_tick = Some(tick);
    let input = ViewerInput {
        left: inputs.pressed(Action::Left),
        right: inputs.pressed(Action::Right),
        up: inputs.pressed(Action::Forward),
        down: inputs.pressed(Action::Back),
        walk: inputs.pressed(Action::Walk),
        jump: inputs.pressed(Action::Jump),
        exit: exit_held,
    };
    let Some(viewer) = screen.viewer.as_mut() else {
        return;
    };
    let was_exiting = viewer.exiting();
    let finished = viewer.update(input);
    let (exiting, pose) = (viewer.exiting(), viewer.pose());
    if exiting && !was_exiting {
        screen.confirm = inputs.pressed(Action::Use) || keys.pressed(KeyCode::Enter);
        // Sound effect 60 plays when the view begins to close.
        cue(60);
    }
    screen.pose = Some(pose);
    if exiting {
        screen.line = Line::Ended;
    } else {
        advance_line(&mut screen, time.elapsed());
    }
    if finished {
        screen.open = false;
        screen.waiting_for_release = true;
        screen.model = None;
        screen.pose = None;
        screen.viewer = None;
        screen.lines = None;
        screen.speech_request = None;
        screen.speech_reply = None;
        if screen.from_inventory {
            screen.inventory_result = Some(screen.confirm);
            screen.from_inventory = false;
        } else if let Some(actors) = actors.as_mut()
            && let Some(world) = actors.current.as_mut()
        {
            world.globals_mut().finish_item_viewer();
        }
        resume(&mut screen, &mut virtual_time, &mut clock);
    }
}

fn default_screen() -> ExamineScreen {
    ExamineScreen::default()
}

/// The viewer, description lines and model for a request; `None` is an item absent from the list.
/// How a view is entered.
#[derive(Clone, Copy)]
enum Entry {
    Scripted { words: [i32; 2] },
    Inventory { row: i32, column: i32 },
}

fn open(
    hud: &crate::script::ScriptHud,
    globals: Option<&crate::script::GameplayGlobals>,
    item_id: i32,
    entry: Entry,
) -> Result<Option<(ItemViewer, DescriptionLines, String)>> {
    let Some(item) = hud.examine_item(item_id as u32)? else {
        return Ok(None);
    };
    ensure!(
        item.exported,
        "item {item_id}: the HUD export predates the inventory model record; export it again",
    );
    let model = item
        .model
        .with_context(|| format!("item {item_id}: ITEM.INI names no inventory_object_file"))?;
    let extents = item
        .extents
        .with_context(|| format!("item {item_id}: inventory model {model} has no extents"))?;
    let viewer = match entry {
        Entry::Scripted { words } => ItemViewer::open(extents, words, ASPECT),
        Entry::Inventory { row, column } => {
            ItemViewer::open_inventory(extents, row, column, ASPECT)
        }
    };
    let potion = usize::try_from((item_id - 20) / 2)
        .ok()
        .and_then(|index| globals?.potion(index))
        .unwrap_or(0);
    let lines = DescriptionLines::new(item_id, item.description.as_deref(), potion);
    Ok(Some((viewer, lines, model)))
}

/// Start the next description line when none is playing, and end the sequence at a label with no
/// speech record. A line ends with its speech clip, measured on the real clock.
fn advance_line(screen: &mut ExamineScreen, now: std::time::Duration) {
    loop {
        match screen.line {
            Line::Idle => {
                let Some(label) = screen.lines.as_mut().and_then(Iterator::next) else {
                    screen.line = Line::Ended;
                    return;
                };
                screen.speech_request = Some(label);
                screen.speech_reply = None;
                screen.line = Line::Requested;
                return;
            }
            Line::Requested => match screen.speech_reply.take() {
                Some(Some(seconds)) => {
                    screen.line = Line::Playing {
                        until: now + std::time::Duration::from_secs_f32(seconds.max(0.0)),
                    };
                    return;
                }
                Some(None) => screen.line = Line::Ended,
                None => return,
            },
            Line::Playing { until } => {
                if now < until {
                    return;
                }
                screen.line = Line::Idle;
            }
            Line::Ended => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELD: ViewerInput = ViewerInput {
        left: false,
        right: false,
        up: false,
        down: false,
        walk: false,
        jump: false,
        exit: false,
    };

    fn viewer() -> ItemViewer {
        ItemViewer::open([100, 60, 40], [0, 0], 0.75)
    }

    fn settle(viewer: &mut ItemViewer) {
        for _ in 0..7 {
            assert!(!viewer.update(HELD));
        }
    }

    #[test]
    fn request_words_offset_the_entry_and_swap_on_the_way_out() {
        let mut viewer = ItemViewer::open([100, 60, 40], [30, 70], 0.75);
        assert_eq!(viewer.pose().position, [30.0, 70.0, 220.0 + 1490.0]);
        settle(&mut viewer);
        viewer.update(ViewerInput { exit: true, ..HELD });
        // At the first exit step the model is a fifth of the way from rest to the swapped
        // target `(70, 30, 1490)`.
        let position = viewer.pose().position;
        assert!((position[0] - 70.0 * 0.15).abs() < 1e-3, "{position:?}");
        assert!((position[1] - 30.0 * 0.15).abs() < 1e-3);
        assert!((position[2] - (220.0 + (1490.0 - 220.0) * 0.15)).abs() < 1e-2);
    }

    #[test]
    fn the_inventory_entry_flies_from_the_cursor_cell_and_back_to_it() {
        let mut viewer = ItemViewer::open_inventory([100, 60, 40], 1, 2, 0.75);
        let cell = [2.0 * 388.0 - 582.0, 1.0 * 378.0 - 432.0];
        let start = viewer.pose().position;
        assert_eq!(start, [cell[0], cell[1], 1490.0]);
        settle(&mut viewer);
        assert_eq!(viewer.pose().position, [0.0, 0.0, 220.0]);
    }

    #[test]
    fn entry_flies_in_along_the_view_axis_with_zero_angles() {
        let mut viewer = viewer();
        assert_eq!(viewer.pose().position, [0.0, 0.0, 220.0 + 1490.0]);
        viewer.update(HELD);
        assert!((viewer.pose().position[2] - (220.0 + 1490.0 * 0.85)).abs() < 1e-2);
        settle(&mut viewer);
        let pose = viewer.pose();
        assert_eq!(pose.position, [0.0, 0.0, 220.0]);
        assert_eq!([pose.turn, pose.pitch, pose.roll], [0, 0, 0]);
    }

    #[test]
    fn plain_directions_turn_and_pitch_eighty_units_and_wrap() {
        let mut viewer = viewer();
        settle(&mut viewer);
        viewer.update(ViewerInput {
            left: true,
            up: true,
            ..HELD
        });
        let pose = viewer.pose();
        assert_eq!([pose.turn, pose.pitch], [80, 2048 - 80]);
        viewer.update(ViewerInput {
            right: true,
            down: true,
            ..HELD
        });
        let pose = viewer.pose();
        assert_eq!([pose.turn, pose.pitch], [0, 0]);
    }

    #[test]
    fn jump_zooms_and_rolls_and_walk_pans_within_the_limits() {
        let mut viewer = viewer();
        settle(&mut viewer);
        let scale = f64::from(180.0_f32) / 980.0 + 0.1;
        viewer.update(ViewerInput {
            jump: true,
            up: true,
            right: true,
            ..HELD
        });
        let pose = viewer.pose();
        assert_eq!(pose.roll, 80);
        assert!((f64::from(pose.position[2]) - (220.0 + 80.0 * scale)).abs() < 1e-3);
        for _ in 0..200 {
            viewer.update(ViewerInput {
                jump: true,
                down: true,
                ..HELD
            });
        }
        assert_eq!(viewer.pose().position[2], 40.0);
        for _ in 0..200 {
            viewer.update(ViewerInput {
                walk: true,
                right: true,
                down: true,
                jump: true,
                ..HELD
            });
        }
        let pose = viewer.pose();
        let scale = f64::from(0.0_f32) + 0.1;
        let half = (200.0 * scale).round_ties_even() as f32;
        assert_eq!(pose.position[0], half);
        assert_eq!(
            pose.position[1],
            (half as f64 * 0.75).round_ties_even() as f32
        );
    }

    #[test]
    fn exit_waits_for_a_release_then_flies_back_to_the_entry_depth() {
        let mut viewer = viewer();
        viewer.update(ViewerInput { exit: true, ..HELD });
        assert!(!viewer.exiting());
        settle(&mut viewer);
        viewer.update(ViewerInput { exit: true, ..HELD });
        assert!(viewer.exiting());
        // Fade 0.15 at the request, then one step per update until it passes one.
        let mut updates = 0;
        while !viewer.update(HELD) {
            updates += 1;
            assert!(updates < 20);
        }
        assert_eq!(updates, 5);
    }

    #[test]
    fn exit_interpolates_angles_back_the_short_way() {
        let mut viewer = viewer();
        settle(&mut viewer);
        for _ in 0..5 {
            viewer.update(ViewerInput { left: true, ..HELD });
        }
        assert_eq!(viewer.pose().turn, 400);
        viewer.update(ViewerInput { exit: true, ..HELD });
        let pose = viewer.pose();
        assert!(pose.turn > 0 && pose.turn < 400, "{}", pose.turn);
    }

    #[test]
    fn description_lines_follow_the_item_range() {
        let lines = |item, description, potion| {
            DescriptionLines::new(item, description, potion)
                .map(|label| String::from_utf8(label.to_vec()).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(lines(20, Some("cibk"), 3), ["cibk", "cip4"]);
        assert_eq!(lines(26, None, 0), ["cip1"]);
        assert_eq!(lines(36, Some("xxxx"), 0), ["cibe", "cibl"]);
        assert_eq!(
            lines(53, None, 0),
            ["iij2", "iij3", "iij4", "iij5", "iij6", "iij7", "iij8"]
        );
        let sap = lines(55, None, 0);
        assert_eq!(&sap[..3], ["cikj", "ny01", "ny02"]);
        assert_eq!(sap.len(), 12);
        assert_eq!(lines(4, Some("cisw"), 0), ["cisw"]);
        assert!(lines(4, None, 0).is_empty());
        // A counter of nine reaches the two-digit tag, which is cut to four characters.
        assert_eq!(lines(22, None, 9), ["cip1"]);
    }
}
