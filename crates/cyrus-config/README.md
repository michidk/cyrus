# cyrus-config

Where cyrus keeps its own state on the player's machine (settings, the remembered game-files
folder, preferences, saves, exported caches and logs) and how it reads and writes it. Nothing is
ever written to the install.

A leaf crate: no workspace dependencies; `directories` and `anyhow` on native targets only. Every
crate that keeps local state asks it for a directory and goes through `read` and `write`, instead
of spelling `cyrus-config/...` or replacing files itself.

```rust
let settings = cyrus_config::config_dir().join("settings.toml");
let cache = cyrus_config::cache_dir();
let logs = cyrus_config::log_dir();

// None when the file does not exist; an error that names the path otherwise.
let saved = cyrus_config::read(&settings)?;
// Temp file, sync, one rename; the accepted document survives a failed write.
cyrus_config::write(&settings, b"scene = \"START\"")?;
```

## Where the root is

Chosen once per process, first match wins:

1. `CYRUS_CONFIG_DIR`, used as given (`cache/` and `logs/` below it).
2. A `cyrus-config/` directory that already exists in the working directory. This is the
   layout of a development checkout, the web server and earlier releases, so existing state keeps
   working; `cache/` and `logs/` below it.
3. The platform directories for an application named `cyrus`:

   | | config | cache | logs |
   |---|---|---|---|
   | Linux | `~/.config/cyrus` | `~/.cache/cyrus` | `~/.local/state/cyrus/logs` |
   | macOS | `~/Library/Application Support/cyrus` | `~/Library/Caches/cyrus` | `~/Library/Application Support/cyrus/logs` |
   | Windows | `%APPDATA%\cyrus\config` | `%LOCALAPPDATA%\cyrus\cache` | `%LOCALAPPDATA%\cyrus\data\logs` |

The browser build has no filesystem and persists through `localStorage`; on `wasm32` the crate
only supplies the checkout-local names so shared code compiles.

```
cargo test -p cyrus-config
```
