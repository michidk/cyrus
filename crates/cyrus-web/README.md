# cyrus-web

The browser build (wasm32). It shows scenes the native game exported to
`cyrus-config/cache/`, served next to the page; it does not read the install
and has no preload or control port.

```sh
just web-build   # cargo build for wasm32 + wasm-bindgen into target/web
just run-web     # serve target/web behind the VibePod preview "cyrus-web"
```

`web/index.html` streams the engine and the scene with a progress bar before
starting Bevy. `?scene=START` or `?scene=27` picks a world (stem or
WORLD.INI index). Reuses `cyrus-game`'s portable modules (`fidelity`,
`player`, `settings`); the modern controller is the default here because the
browser has no original keyboard scheme to be faithful to yet.

Not a default workspace member so day-to-day builds and tests stay lean.
