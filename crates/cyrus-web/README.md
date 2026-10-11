# cyrus-web

The browser build (wasm32). Default launch shows startup through the book menu.
Export its assets with `cargo run -p cyrus -- --export-startup`. With `?scene=...`, it shows scenes the native game exported to
`cyrus-config/cache/`, served next to the page; it does not read the install
and has no preload or control port. Movies plays the selected installed cinematic
from `cyrus-config/cache/movies/`, exported by `--export-startup`. Video decodes
in-process; playback shares native subtitle, interlace, skip, and menu-return rules.

```sh
just web-build   # cargo build for wasm32 + wasm-bindgen into target/web
just run-web     # serve target/web behind the VibePod preview "cyrus-web"
```

`web/index.html` streams the engine before starting Bevy. Direct scene launches
also download the scene with a progress bar. `?scene=START` or `?scene=27` picks a world (stem or
WORLD.INI index). It shares `cyrus-world`, `cyrus-game` and `cyrus-render`
plugins with native. This host owns URL/DOM world selection and localStorage
persistence. Direct scene launches wait for exported keyboard defaults and apply
saved bindings; controller fidelity defaults to Faithful.
With a scene URL, press F1 to choose a world, set flycam speed, and change runtime fidelity.

Not a default workspace member so day-to-day builds and tests stay lean.
