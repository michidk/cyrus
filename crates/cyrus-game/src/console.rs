//! Shared console editor and hidden gameplay entry from preservation's console rules.
use crate::{AppState, cheats::Cheats};
use bevy::{
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Line {
    pub text: String,
    pub report: bool,
}

#[derive(Resource, Serialize)]
pub struct Console {
    pub open: bool,
    /// Another host UI owns keyboard input; events are drained while blocked.
    #[serde(skip)]
    pub blocked: bool,
    pub lines: Vec<Line>,
    pub selected: usize,
    pub cursor: usize,
    pub top: usize,
    pub page_lines: usize,
    #[serde(skip)]
    recall: usize,
    #[serde(skip)]
    hidden: String,
    #[serde(skip)]
    held: bool,
}
impl Default for Console {
    fn default() -> Self {
        Self {
            open: false,
            blocked: false,
            lines: vec![
                Line {
                    text: "RedGuard Console".into(),
                    report: true,
                },
                Line {
                    text: String::new(),
                    report: false,
                },
            ],
            selected: 1,
            cursor: 0,
            top: 0,
            page_lines: 40,
            recall: 1,
            hidden: String::new(),
            held: false,
        }
    }
}
impl Console {
    fn append(&mut self, line: Line) {
        self.lines.push(line);
        if self.lines.len() > 512 {
            self.lines.remove(0);
        }
        self.selected = self.lines.len() - 1;
        self.cursor = 0;
        self.recall = self.selected;
        self.reveal();
    }
    fn reveal(&mut self) {
        self.cursor = self.cursor.min(self.lines[self.selected].text.len());
        if self.selected < self.top {
            self.top = self.selected;
        }
        if self.selected >= self.top + self.page_lines.max(1) {
            self.top = self.selected + 1 - self.page_lines.max(1);
        }
    }
    pub fn report(&mut self, text: impl Into<String>) {
        self.append(Line {
            text: text.into(),
            report: true,
        });
        self.append(Line {
            text: String::new(),
            report: false,
        });
    }
    /// All input sources use the same dispatcher and output history.
    pub fn submit(&mut self, cheats: &mut Cheats, text: &str) {
        if !text.is_ascii() || text.len() > 100 {
            self.report("Console commands must contain at most 100 ASCII characters.");
            return;
        }
        if self.lines.last().is_some_and(|line| {
            !line.report && (line.text.is_empty() || self.selected == self.lines.len() - 1)
        }) {
            self.lines.pop();
        }
        self.append(Line {
            text: text.into(),
            report: false,
        });
        if matches!(
            text.split([';', '\n', '\r'])
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "console clear" | "con clear"
        ) {
            self.lines.clear();
            self.selected = 0;
            self.top = 0;
        } else {
            let output = crate::cheat_command::execute(cheats, text)
                .unwrap_or_else(|error| vec![format!("{error:#}")]);
            for text in output {
                self.append(Line { text, report: true });
            }
        }
        self.append(Line {
            text: String::new(),
            report: false,
        });
    }
    pub fn key(&mut self, key: KeyCode, text: Option<&str>, cheats: &mut Cheats) {
        let editable = !self.lines[self.selected].report;
        match key {
            KeyCode::F12 | KeyCode::Escape => self.open = false,
            KeyCode::Enter | KeyCode::NumpadEnter if editable => {
                let line = self.lines[self.selected].text.clone();
                self.submit(cheats, &line);
            }
            KeyCode::ArrowUp => self.selected = self.selected.saturating_sub(1),
            KeyCode::ArrowDown => self.selected = (self.selected + 1).min(self.lines.len() - 1),
            KeyCode::ArrowLeft => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::ArrowRight => {
                self.cursor = (self.cursor + 1).min(self.lines[self.selected].text.len())
            }
            KeyCode::Home => {
                if self.cursor == 0 {
                    self.selected = 0;
                }
                self.cursor = 0;
            }
            KeyCode::End => {
                let end = self.lines[self.selected].text.len().saturating_sub(1);
                if self.cursor == end {
                    self.selected = self.lines.len() - 1;
                } else {
                    self.cursor = end;
                }
            }
            KeyCode::PageUp => {
                self.selected = self
                    .selected
                    .saturating_sub(self.page_lines.saturating_sub(2).max(1))
            }
            KeyCode::PageDown => {
                self.selected = (self.selected + self.page_lines.saturating_sub(2).max(1))
                    .min(self.lines.len() - 1)
            }
            KeyCode::Tab if editable => {
                if let Some(index) = (0..self.recall)
                    .rev()
                    .find(|index| !self.lines[*index].report)
                {
                    self.lines[self.selected].text = self.lines[index].text.clone();
                    self.cursor = self.lines[self.selected].text.len();
                    self.recall = index;
                } else {
                    self.lines[self.selected].text.clear();
                    self.cursor = 0;
                    self.recall = self.selected;
                }
            }
            KeyCode::Backspace if editable && self.cursor > 0 => {
                self.cursor -= 1;
                self.lines[self.selected].text.remove(self.cursor);
            }
            KeyCode::Delete if editable && self.cursor < self.lines[self.selected].text.len() => {
                self.lines[self.selected].text.remove(self.cursor);
            }
            _ if editable => {
                for ch in text
                    .unwrap_or_default()
                    .chars()
                    .filter(|ch| ch.is_ascii() && !ch.is_ascii_control())
                {
                    if self.lines[self.selected].text.len() >= 100 {
                        break;
                    }
                    self.lines[self.selected].text.insert(self.cursor, ch);
                    self.cursor += 1;
                }
            }
            _ => {}
        }
        self.reveal();
    }
    /// Gameplay has a release latch and a separate 127-byte buffer; no editing commands.
    pub fn gameplay(&mut self, key: KeyCode, text: Option<&str>, cheats: &mut Cheats) {
        if self.held {
            return;
        }
        self.held = true;
        if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter) {
            let name = std::mem::take(&mut self.hidden);
            if cheats
                .state()
                .ok()
                .and_then(|state| state.value(&name))
                .is_some()
                && let Err(error) = cheats.execute(&name, None)
            {
                log::error!("cheat {name}: {error:#}");
            }
        } else {
            for ch in text.unwrap_or_default().bytes().filter(u8::is_ascii) {
                if self.hidden.len() < 127 {
                    self.hidden.push(ch.to_ascii_lowercase() as char);
                }
            }
        }
    }
    pub fn release(&mut self) {
        self.held = false;
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConsoleInput;

pub struct ConsolePlugin;
impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Console>()
            .add_message::<KeyboardInput>()
            .add_systems(PreUpdate, input.in_set(ConsoleInput).after(InputSystems));
    }
}
fn input(
    mut events: MessageReader<KeyboardInput>,
    state: Res<State<AppState>>,
    mut console: ResMut<Console>,
    mut cheats: ResMut<Cheats>,
    mut keys: Option<ResMut<ButtonInput<KeyCode>>>,
) {
    if *state.get() != AppState::InGame || console.blocked {
        events.clear();
        console.open = false;
        console.hidden.clear();
        console.release();
        return;
    }
    let was_open = console.open;
    for event in events.read() {
        if event.state == ButtonState::Released {
            console.release();
            continue;
        }
        let text = match &event.logical_key {
            Key::Character(text) => Some(text.as_str()),
            Key::Space => Some(" "),
            _ => None,
        };
        if !console.open && event.key_code == KeyCode::F12 && !event.repeat {
            console.open = true;
            console.hidden.clear();
        } else if console.open
            && !(event.repeat && matches!(event.key_code, KeyCode::F12 | KeyCode::Escape))
        {
            console.key(event.key_code, text, &mut cheats);
        } else if !event.repeat {
            console.gameplay(event.key_code, text, &mut cheats);
        }
    }
    if (was_open || console.open)
        && let Some(keys) = keys.as_mut()
    {
        keys.reset_all();
    }
}
