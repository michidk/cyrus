//! Paint the installed book's page surfaces: labels, widgets and pictures drawn into the textures
//! the book's 3D scene (`book_scene`) shows.
use super::menu::MenuState;
use anyhow::{Context, ensure};
use cyrus_assets::menu::MenuAsset;
use cyrus_assets::menu::PageTexture;
use cyrus_assets::menu::{MenuEntry, MenuItem, MenuPage, MenuPicture};
use cyrus_assets::preferences::MenuPreferences;
use image::RgbaImage;

/// What a menu redraw produced: the painted page surfaces of the book's 3D scene, or a dialog over it.
pub(super) enum Painted {
    /// Every page surface, text, widgets and pictures included, ready to upload as textures.
    Surfaces(Vec<PageTexture>),
    /// A confirmation or description dialog, transparent outside the dialog, drawn over the scene.
    Dialog(RgbaImage),
}

/// What one redraw of one book page reads.
#[derive(Clone, Copy)]
struct PageContext<'a> {
    book: &'a MenuAsset,
    state: &'a MenuState,
    preferences: &'a MenuPreferences,
    id: u8,
    page: &'a MenuPage,
}

pub(super) fn paint(book: &MenuAsset, state: &MenuState) -> anyhow::Result<Painted> {
    if let Some(confirmation) = &state.delete_confirmation {
        return confirmation.paint(book).map(Painted::Dialog);
    }
    if let Some(editor) = &state.description_editor {
        return super::confirmation::LastGameConfirmation::paint_description(
            &book.confirmation,
            &editor.background,
            &editor.display(),
        )
        .map(Painted::Dialog);
    }
    let preferences = state
        .preferences
        .as_ref()
        .context("menu: preferences unavailable")?;
    let mut surfaces = book.textures.clone();
    for (id, page) in &book.pages {
        let context = PageContext {
            book,
            state,
            preferences,
            id: *id,
            page,
        };
        for index in 0..page.entries.len() {
            paint_entry(&context, &mut surfaces, index)?;
        }
        paint_thumbnail(&context, &mut surfaces);
        paint_movie_preview(&context, &mut surfaces);
    }
    Ok(Painted::Surfaces(surfaces))
}

/// The entry as the page shows it: the live entry on the open page, the Save and Load row labels
/// from the stored slots, and the first Save row as `<NEW SAVE GAME>`.
fn entry_item(context: &PageContext, index: usize) -> MenuItem {
    let PageContext {
        state, id, page, ..
    } = *context;
    let entry = &page.entries[index];
    let mut item = if id == state.page {
        state.entries.get(index).unwrap_or(&entry.item).clone()
    } else {
        entry.item.clone()
    };
    if id == 1 && item.action == 101 && state.list_page == 0 {
        item.text = "<NEW SAVE GAME>".into();
    }
    if matches!(id, 1 | 2) && matches!(item.action, 102 | 202) {
        let row = page.entries[..index]
            .iter()
            .filter(|entry| matches!(entry.item.action, 102 | 202))
            .count();
        let base = if id == 1 {
            if state.list_page == 0 {
                0
            } else {
                8 + (state.list_page - 1) * 9
            }
        } else {
            state.list_page * 9
        };
        let slot = state
            .slots
            .iter()
            .filter(|slot| id != 1 || (slot.slot != 0 && slot.slot != 1))
            .skip(base)
            .nth(row);
        item.text = slot
            .map(|slot| slot.description.chars().take(40).collect())
            .unwrap_or_default();
        item.grayed = slot.is_none();
    }
    item
}

/// Draws one entry's label, its value widget and its key binding into the page surface it uses.
fn paint_entry(
    context: &PageContext,
    surfaces: &mut [PageTexture],
    index: usize,
) -> anyhow::Result<()> {
    let PageContext {
        book,
        state,
        id,
        page,
        ..
    } = *context;
    let entry = &page.entries[index];
    let item = entry_item(context, index);
    if !item.visible {
        return Ok(());
    }
    let disabled = state.main_entry_disabled(id, index);
    let selected =
        state.selections.get(&id).copied().flatten() == Some(index) && item.eligible() && !disabled;
    // The original draws the Save and Load list rows with the small font pair.
    let small = matches!(item.action, 101 | 102 | 202);
    let font = &book.fonts[usize::from(selected) + 2 * usize::from(small)];
    let style = if selected {
        0
    } else if !item.selectable {
        3
    } else if item.grayed || disabled {
        2
    } else {
        1
    };
    let surface = &mut surfaces[entry.texture];
    text(
        surface,
        font,
        &book.palette,
        &item.text,
        entry.position,
        entry.justify,
        style,
    )?;
    paint_value(context, surface, entry);
    if (710..=724).contains(&entry.item.action) {
        paint_binding(context, surface, entry)?;
    }
    Ok(())
}

/// The check or slider picture of an entry that has a stored value.
fn paint_value(context: &PageContext, surface: &mut PageTexture, entry: &MenuEntry) {
    let PageContext {
        book, preferences, ..
    } = *context;
    let Some(value) = preferences.values.get(&entry.item.action) else {
        return;
    };
    let slider = matches!(entry.item.action, 601 | 602);
    let frames = if slider { &book.sliders } else { &book.checks };
    let frame = if slider {
        ((*value as f32 * (frames.len() - 1) as f32 / (entry.slider[1] - entry.slider[0]) as f32)
            .round() as usize)
            .min(frames.len() - 1)
    } else {
        *value as usize
    };
    picture(surface, &frames[frame], entry.output_position);
}

/// The name of a Controls entry's key, or the blinking capture prompt while it is being rebound.
fn paint_binding(
    context: &PageContext,
    surface: &mut PageTexture,
    entry: &MenuEntry,
) -> anyhow::Result<()> {
    let PageContext {
        book,
        state,
        preferences,
        id,
        ..
    } = *context;
    let capturing = state.page == id && state.capturing == Some(entry.item.action);
    let prompt = if state.blink_on {
        format!("<{}>", book.waiting_message)
    } else {
        format!(">{}<", book.waiting_message)
    };
    let label = if capturing {
        &prompt
    } else {
        &book.key_names[usize::from(preferences.bindings[(entry.item.action - 710) as usize])]
    };
    let font = &book.fonts[usize::from(capturing)];
    let shown = drawn_characters(
        font,
        &label.to_ascii_uppercase(),
        entry.output_position[0],
        (entry.output_justify == 0).then_some(BINDING_PEN_LIMIT),
    );
    text(
        surface,
        font,
        &book.palette,
        label.get(..shown).unwrap_or(label),
        entry.output_position,
        entry.output_justify,
        if capturing { 0 } else { 1 },
    )
}

/// The snapshot or save thumbnail the page shows for its selection, as encoded image bytes.
fn selected_thumbnail<'a>(context: &PageContext<'a>) -> Option<&'a Vec<u8>> {
    let PageContext { state, id, .. } = *context;
    if matches!(id, 1 | 2) && id == state.page {
        state.selected.and_then(|index| {
            if state
                .entries
                .get(index)
                .is_some_and(|entry| entry.action == 101)
            {
                state.gameplay_thumbnail.as_ref()
            } else {
                state
                    .slot_rows
                    .get(&index)
                    .and_then(|row| state.slots.get(*row))
                    .and_then(|slot| slot.thumbnail.as_ref())
            }
        })
    } else if id == 0 {
        state.gameplay_thumbnail.as_ref()
    } else {
        None
    }
}

/// Pastes the selected 256 by 192 thumbnail into the page's picture frame.
fn paint_thumbnail(context: &PageContext, surfaces: &mut [PageTexture]) {
    let Some(thumbnail) = selected_thumbnail(context) else {
        return;
    };
    let Ok(decoded) = image::load_from_memory(thumbnail) else {
        return;
    };
    let rgba = decoded.to_rgba8();
    if rgba.width() == 256
        && rgba.height() == 192
        // The main page keeps its entries on its second surface, so its snapshot takes the first;
        // the save/load pages show theirs on the second.
        && let Some(surface) = context.page.surfaces.get(usize::from(context.id != 0))
    {
        picture(
            &mut surfaces[*surface],
            &MenuPicture {
                width: 256,
                height: 192,
                rgba: rgba.into_raw(),
            },
            [0, 33],
        );
    }
}

/// Shows the preview frame of the selected movie on the Movies page.
fn paint_movie_preview(context: &PageContext, surfaces: &mut [PageTexture]) {
    let PageContext {
        book,
        state,
        id,
        page,
        ..
    } = *context;
    if id != 3 {
        return;
    }
    let Some(index) = state.selections.get(&id).copied().flatten() else {
        return;
    };
    let action = page.entries[index].item.action;
    if (301..=305).contains(&action)
        && let Some(frame) = book.movies.get((action - 301) as usize)
        && let Some(surface) = page.surfaces.get(1)
    {
        picture(&mut surfaces[*surface], frame, [0, 33]);
    }
}

fn picture(surface: &mut PageTexture, picture: &MenuPicture, position: [i32; 2]) {
    for y in 0..picture.height {
        for x in 0..picture.width {
            let dx = position[0] + i32::from(x);
            let dy = position[1] + i32::from(y);
            if dx < 0 || dy < 0 || dx >= i32::from(surface.width) || dy >= i32::from(surface.height)
            {
                continue;
            }
            let source = (usize::from(y) * usize::from(picture.width) + usize::from(x)) * 4;
            if picture.rgba[source + 3] == 0 {
                continue;
            }
            let target = (dy as usize * usize::from(surface.width) + dx as usize) * 4;
            surface.rgba[target..target + 4].copy_from_slice(&picture.rgba[source..source + 4]);
        }
    }
}

/// Controls binding names draw no glyph whose pen starts at or beyond this column of
/// the 256-pixel page surface. **Verified:** original captures of `NUM MINUS`, `LEFT MOUSE`,
/// `RIGHT MOUSE` and wide/narrow test names (pen 246 drawn, 247 omitted).
const BINDING_PEN_LIMIT: i32 = 247;

/// Number of leading characters of `label` drawn from pen `start`: glyphs stop at the first
/// whose pen reaches `limit`.
fn drawn_characters(font: &cyrus_text::Font, label: &str, start: i32, limit: Option<i32>) -> usize {
    let mut pen = start;
    for (index, ch) in label.bytes().enumerate() {
        if limit.is_some_and(|limit| pen >= limit) {
            return index;
        }
        pen += font.advance(ch);
    }
    label.len()
}

fn text(
    surface: &mut PageTexture,
    font: &cyrus_text::Font,
    palette: &[[u8; 3]],
    label: &str,
    position: [i32; 2],
    justify: u8,
    style: u8,
) -> anyhow::Result<()> {
    ensure!(
        label.is_ascii(),
        "menu: unsupported non-ASCII text {label:?}"
    );
    let label = label.to_ascii_uppercase();
    if let Some(ch) = label.bytes().find(|&ch| font.glyph(ch).is_none()) {
        anyhow::bail!("menu: text {label:?} has unsupported glyph {ch}");
    }
    let width = font.width(&label);
    let x = position[0]
        - match justify {
            1 => width / 2,
            2 => width,
            _ => 0,
        };
    font.draw(&label, [x, position[1]], |dx, dy, pixel| {
        if dx < 0 || dy < 0 || dx >= i32::from(surface.width) || dy >= i32::from(surface.height) {
            return;
        }
        let offset = dy as usize * usize::from(surface.width) + dx as usize;
        let index = match style {
            0 => pixel,
            2 => surface.indices[offset].wrapping_add(4),
            3 => (pixel & 15) + 144,
            _ => pixel.wrapping_add(5),
        };
        surface.indices[offset] = index;
        let [r, g, b] = palette[usize::from(index)];
        surface.rgba[offset * 4..offset * 4 + 4].copy_from_slice(&[r, g, b, 255]);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_names_stop_at_the_first_glyph_whose_pen_reaches_the_limit() {
        let font = cyrus_text::Font {
            start: u16::from(b'A'),
            line_height: 1,
            glyphs: vec![cyrus_text::Glyph {
                enabled: true,
                width: 20,
                height: 1,
                offset: [0, 0],
                pixels: vec![1; 20],
            }],
            palette: Vec::new(),
        };
        // Advance 21: pens 120, 141 ... 246 (draws), 267 (omitted).
        assert_eq!(drawn_characters(&font, "AAAAAAAAAA", 120, Some(247)), 7);
        assert_eq!(drawn_characters(&font, "AAAAAAAAAA", 120, Some(246)), 6);
        assert_eq!(drawn_characters(&font, "AAAAAAAAAA", 120, None), 10);
        assert_eq!(drawn_characters(&font, "AAA", 120, Some(247)), 3);
    }
}
