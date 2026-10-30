//! Shared console editor and hidden gameplay entry from preservation's console rules.
use crate::Source;
use bevy::prelude::*;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Line {
    pub text: String,
    pub report: bool,
}

#[derive(Resource, Serialize)]
pub struct Console {
    pub preferences: crate::Preferences,
    pub preference_error: Option<String>,
    #[serde(skip)]
    pub(crate) store: Option<Box<dyn crate::Store>>,
    /// GUI indices for the current world, supplied by presentation.
    #[serde(skip)]
    pub palette: Option<[u8; 16]>,
    pub open: bool,
    /// The last close was by Escape (not F12); read once by [`Console::take_escape`].
    #[serde(skip)]
    escaped: bool,
    #[serde(skip)]
    blink_tick: u32,
    #[serde(skip)]
    blink_remainder: u32,
    #[serde(skip)]
    blink_phase: bool,
    #[serde(skip)]
    pub(crate) cursor_forced: bool,
    #[serde(skip)]
    pub enabled: bool,
    #[serde(skip)]
    pub(crate) pending: std::collections::VecDeque<(String, Source)>,
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
            preferences: default(),
            preference_error: None,
            store: None,
            palette: None,
            open: false,
            escaped: false,
            blink_tick: 0,
            blink_remainder: 0,
            blink_phase: false,
            cursor_forced: false,
            enabled: true,
            pending: default(),
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
    /// Restore once from host storage; malformed documents remain visible errors.
    pub fn with_store(store: impl crate::Store) -> Self {
        let mut console = Self::default();
        match store.load() {
            Ok(Some(mut preferences)) => {
                if preferences.colors.iter().any(|index| *index >= 16) {
                    console.preference_error =
                        Some("REDGUARD.CPF: unsupported GUI color index".into());
                }
                preferences.hex_lines = preferences.hex_lines.max(1);
                // A nonzero saved shadow retains the existing state, initially disabled.
                preferences.shadow = 0;
                console.preferences = preferences;
            }
            Ok(None) => {}
            Err(error) => console.preference_error = Some(format!("REDGUARD.CPF: {error:#}")),
        }
        if let Some(error) = console.preference_error.clone() {
            console.report(error);
        }
        console.store = Some(Box::new(store));
        console
    }

    /// Advance the normal console clock with the host's monotonic BIOS-tick sample.
    pub fn tick(&mut self, tick: u32) {
        let elapsed = tick.wrapping_sub(self.blink_tick) as i32;
        self.blink_tick = tick;
        self.blink_remainder = (i64::from(self.blink_remainder) + i64::from(elapsed)).max(0) as u32;
        if self.blink_remainder >= 18 {
            self.blink_remainder %= 18;
            self.blink_phase = !self.blink_phase;
        }
    }
    pub fn cursor_visible(&self) -> bool {
        self.cursor_forced || self.blink_phase
    }
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
    pub(crate) fn enter(&mut self, text: &str) {
        if self.lines.last().is_some_and(|line| {
            !line.report && (line.text.is_empty() || self.selected == self.lines.len() - 1)
        }) {
            self.lines.pop();
        }
        self.append(Line {
            text: text.into(),
            report: false,
        });
    }
    pub(crate) fn finish(&mut self, output: Vec<String>) {
        for text in output {
            self.append(Line { text, report: true });
        }
        self.append(Line {
            text: String::new(),
            report: false,
        });
    }
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.selected = 0;
        self.top = 0;
        self.cursor = 0;
        self.recall = 0;
    }
    /// Queue interactive input for the next command-dispatch phase.
    pub fn submit(&mut self, text: &str) {
        self.pending.push_back((text.into(), Source::Interactive));
    }
    pub fn key(&mut self, key: KeyCode, text: Option<&str>) {
        self.cursor_forced = true;
        let editable = !self.lines[self.selected].report;
        match key {
            KeyCode::F12 => self.open = false,
            KeyCode::Escape => {
                self.open = false;
                self.escaped = true;
            }
            KeyCode::Enter | KeyCode::NumpadEnter if editable => {
                let line = self.lines[self.selected].text.clone();
                self.submit(&line);
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
    pub fn gameplay(&mut self, key: KeyCode, text: Option<&str>) {
        if self.held {
            return;
        }
        self.held = true;
        if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter) {
            let name = std::mem::take(&mut self.hidden);
            self.pending.push_back((name, Source::Gameplay));
        } else {
            for ch in text.unwrap_or_default().bytes().filter(u8::is_ascii) {
                if self.hidden.len() < 127 {
                    self.hidden.push(ch.to_ascii_lowercase() as char);
                }
            }
        }
    }
    /// Whether the console was last closed with Escape; clears the record.
    pub fn take_escape(&mut self) -> bool {
        std::mem::take(&mut self.escaped)
    }
    pub(crate) fn reset_input(&mut self) {
        self.open = false;
        self.hidden.clear();
        self.release();
    }
    pub fn release(&mut self) {
        self.held = false;
    }
}
