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
}

/// Explicit reading session built from accepted logbook entries, never install writes.
/// Rules follow preservation's `docs/engine/hud.md` logbook reading section.
/// This core does not open a game screen, resolve keys, pause gameplay or draw artwork.
pub struct Logbook {
    lines: Vec<LogbookLine>,
    offset: u32,
    maximum: u32,
    open: bool,
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
        })
    }

    /// Laid-out lines in entry order, including lines outside the current viewport.
    pub fn lines(&self) -> &[LogbookLine] {
        &self.lines
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
        let step = if input.page_up || input.page_down {
            50
        } else {
            10
        };
        if input.up || input.page_up {
            self.offset = self.offset.saturating_sub(step);
        }
        if input.down || input.page_down {
            self.offset = self.offset.saturating_add(step).min(self.maximum);
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
