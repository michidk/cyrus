# cyrus-text

Redguard FNT text for every screen that draws it: the book menu, the title
screen, movie and dialogue subtitles, and the in-game HUD. It has no Bevy
dependency and compiles on wasm; the native build converts a parsed
`rgpre` FNT file with `Font::from`.

`Font` holds the glyph metrics, palette-indexed glyph pixels and the font's
6-bit palette, and serialises into exported caches. It provides the shared
rules from redguard-preservation's FNT and subtitle documentation:

- `glyph`: enabled glyph for a character code.
- `advance` and `width`: glyph width + 1, plus the width of `!` for a space.
- `wrap`: the dialogue line breaking rule.
- `draw`: calls back with every opaque pixel (index 0 is transparent), so each
  caller decides the target and colour: palette-indexed menu pages, flat-colour
  title and movie text, or RGBA overlays.
- `draw_shadowed`: the NPC subtitle style used by dialogue and the HUD, a black
  copy at (+1, +1) beneath each glyph, alpha from `font_alpha` (`[3dfx] font_*`).
- `colour`: palette entries shifted left by 2, matching captured original colours.

Callers keep what differs between screens: justification, wrap width or the
movie character-count rule, colour source, and where pixels land.

```sh
cargo test -p cyrus-text
```
