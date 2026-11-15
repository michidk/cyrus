# cyrus-debug

The Backtick (`` ` ``) debug menu shared by the native and browser game. It
switches fidelity areas between Faithful and Modern (one by one or all at once),
sets render distance, loads an installed world from a dropdown, and opens
separate windows: a `bevy-inspector-egui` World Inspector, Performance,
Day/night and Effects. It depends on `cyrus-app` (egui setup), `cyrus-render`,
`cyrus-game`, `cyrus-script` and `cyrus-console`. It does not read the install
or the browser DOM; hosts fill its world list. Headless launches do not register it.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | `DebugMenuPlugin`, the Backtick menu, fidelity and render-distance controls, `WorldPicker`, inspector window |
| [performance](src/performance.rs) | Frame-rate and frame-time plots over the last 30 seconds, render statistics |
| [day_night](src/day_night.rs) | Modern lighting time of day: cycle, hour, speed |
| [effects](src/effects.rs) | Modern lighting optional effects and their parameters |

## Usage

```rust
app.add_plugins(cyrus_debug::DebugMenuPlugin);
```

Hosts populate `WorldPicker.options` with WORLD.INI IDs and map names and
consume `WorldPicker.pending`. A selection enters through the shared loading and
player-entry path (see [cyrus-startup](../cyrus-startup/README.md)) at marker
zero and clears the inspection camera. Player-facing use of the menu is in
[SETUP](../../docs/SETUP.md).

## Notes

- Every switch here selects a fidelity `Mode` per `Area`
  (`crates/cyrus-game/src/fidelity.rs`); Faithful stays the default. Changing
  render distance selects Modern Fog, and **Original** returns Fog to Faithful.
- Day/night and Effects need Modern lighting and offer to switch it. The browser
  build has no Modern lighting and says so.
- Water and free flight are native mods (`cyrus-mods`, `mods/`), not menu
  controls. The menu never moves the player.
- F1 and F10 stay reserved for the original's version dialog and credits
  ([fixed gameplay shortcuts](https://github.com/michidk/redguard-preservation/blob/c91d64c19d5b38b280e8253eec3ab779fe8810fe/docs/config/keys-ini.md#fixed-gameplay-shortcuts)).
- The overlay camera persists while startup and world cameras are replaced, so
  the menu works at startup, in the book and in a world.
- The inspector edits only types with registered Bevy reflection.

## Testing

```sh
cargo test -p cyrus-debug
just verify debug
```

The tests run headless and do not need an install.
