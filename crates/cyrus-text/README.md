# cyrus-text

Redguard FNT text for every screen that draws it: the book menu, the title
screen, movie and dialogue subtitles, and the in-game HUD. It has no Bevy
dependency and compiles on wasm. Used by `cyrus-assets`, `cyrus-render` and
`cyrus-startup`.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `Font` and `Glyph` (serialisable into exported caches), layout, drawing and `FontStyle` |

## Usage

The `native` feature adds `Font::from(&FntFile)` to convert a font parsed by
`rgpre`. `Font` provides the shared rules from redguard-preservation's FNT and
subtitle documentation:

- `glyph`, `advance`, `width`: enabled-glyph lookup and pen advance
  (glyph width + 1, plus the width of `!` for a space).
- `wrap`: the dialogue line-breaking rule.
- `draw` / `draw_char`: call back with every opaque pixel (index 0 is
  transparent), so the caller decides target and colour.
- `draw_shadowed` (and `_by`, `_wrapped`): the NPC subtitle and HUD style into
  RGBA, a black shadow copy beneath each glyph, colour scale and alpha from `FontStyle`
  (`SYSTEM.INI` `[3dfx] font_*`).
- `colour`: font palette entries expanded from 6 to 8 bits.

## Notes

- Callers keep what differs between screens: justification, wrap width or the
  movie character-count rule, colour source, and where pixels land.
- Alpha 254 and 255 replace the destination opaquely; lower values blend.

## Testing

```sh
cargo test -p cyrus-text
```
