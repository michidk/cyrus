# cyrus-config

Where cyrus keeps its own state on the player's machine (settings, the
remembered install folder, preferences, saves, screenshots, exported caches and
logs) and the one way it reads and writes it. Nothing is ever written to the
install. A leaf crate: no workspace dependencies, and only `directories` and
`anyhow` on native targets. Every crate that keeps local state asks it for a
directory and goes through `read` and `write` instead of spelling
`game-config/...` or writing files itself.

## Modules

| Module | Responsibility |
| --- | --- |
| [lib](src/lib.rs) | Root selection: `config_dir`, `cache_dir`, `log_dir`, `screenshots_dir` |
| [file](src/file.rs) | `read`, `write`, `create_dir_all` with path-naming errors |

## Usage

```rust
let settings = cyrus_config::config_dir().join("settings.toml");
// None when the file does not exist; an error that names the path otherwise.
let saved = cyrus_config::read(&settings)?;
// Creates parent directories and overwrites the document.
cyrus_config::write(&settings, b"scene = \"START\"")?;
```

## Where the root is

Chosen once per process, first match wins:

1. `CYRUS_CONFIG_DIR`, used as given (`cache/` and `logs/` below it).
2. A `game-config/` directory that already exists in the working directory (a
   development checkout, the web server, earlier releases); `cache/`, `logs/`
   and `screenshots/` below it.
3. The platform directories for an application named `cyrus`:

   | | config | cache | logs |
   |---|---|---|---|
   | Linux | `~/.config/cyrus` | `~/.cache/cyrus` | `~/.local/state/cyrus/logs` |
   | macOS | `~/Library/Application Support/cyrus` | `~/Library/Caches/cyrus` | `~/Library/Application Support/cyrus/logs` |
   | Windows | `%APPDATA%\cyrus\config` | `%LOCALAPPDATA%\cyrus\cache` | `%LOCALAPPDATA%\cyrus\data\logs` |

## Notes

- Writes overwrite the destination directly. An interrupted write may leave a
  partial file; atomic replacement is not promised.
- The browser build has no filesystem and persists through `localStorage`; on
  `wasm32` the crate only supplies the checkout-local names so shared code compiles.

## Testing

```sh
cargo test -p cyrus-config
```
