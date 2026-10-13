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
    pub open: bool,
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
            open: false,
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
        let editable = !self.lines[self.selected].report;
        match key {
            KeyCode::F12 | KeyCode::Escape => self.open = false,
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
    pub(crate) fn reset_input(&mut self) {
        self.open = false;
        self.hidden.clear();
        self.release();
    }
    pub fn release(&mut self) {
        self.held = false;
    }
}
