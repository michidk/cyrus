//! Console and rotating-file logging for the native applications.
//!
//! The console and the log file have independent filters in `tracing` syntax
//! (`info,cyrus_game=debug`). `RUST_LOG` sets the console filter like it does for any Bevy
//! application. The file keeps one file per UTC day, `<name>.<date>.log`, and deletes all but
//! the newest [`KEPT_FILES`]. Writes are unbuffered, so a crash loses nothing.
//!
//! The file lives under `cyrus-config/logs` by default, never in the install. The browser build
//! keeps Bevy's console logging.
use anyhow::Context;
use bevy::log::tracing_subscriber::{EnvFilter, Layer, filter::LevelFilter, fmt};
use bevy::log::{BoxedFmtLayer, BoxedLayer, Level, LogPlugin, error, info};
use bevy::prelude::{App, Resource};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

/// Directory of the log files, relative to the working directory like the other local state.
pub const DEFAULT_DIR: &str = "cyrus-config/logs";
pub const DEFAULT_CONSOLE_FILTER: &str = "info";
/// A debug file keeps what a shared log needs for diagnosis; bare `debug` would add every dependency.
pub const DEFAULT_FILE_FILTER: &str = "info,cyrus=debug";
/// Daily files kept; older ones are deleted when a file opens or rotates.
pub const KEPT_FILES: usize = 7;

/// Log options shared by the native applications' command lines.
#[derive(clap::Args, Debug, Clone)]
pub struct LogArgs {
    /// Console log filter, for example `warn` or `info,cyrus_game=debug`.
    #[arg(long = "log", env = "RUST_LOG", default_value = DEFAULT_CONSOLE_FILTER, value_parser = filter)]
    pub console: String,
    /// Log file filter. `off` disables the file.
    #[arg(long = "log-file", env = "CYRUS_LOG_FILE", default_value = DEFAULT_FILE_FILTER, value_parser = filter)]
    pub file: String,
    /// Directory for the daily log files.
    #[arg(long = "log-dir", env = "CYRUS_LOG_DIR", default_value = DEFAULT_DIR)]
    pub dir: PathBuf,
}

/// The settings `LogPlugin`'s function-pointer hooks read from the app.
#[derive(Resource, Clone)]
struct Sinks {
    console: String,
    file: String,
    dir: PathBuf,
    name: &'static str,
}

fn filter(value: &str) -> Result<String, String> {
    EnvFilter::try_new(value)
        .map(|_| value.to_owned())
        .map_err(|error| error.to_string())
}

/// A blank value, such as an empty `RUST_LOG`, means the default. Values were validated by clap.
fn parse(value: &str, default: &str) -> EnvFilter {
    let value = if value.trim().is_empty() {
        default
    } else {
        value
    };
    EnvFilter::new(value)
}

fn console_filter(value: &str) -> EnvFilter {
    parse(value, DEFAULT_CONSOLE_FILTER)
}

fn file_filter(value: &str) -> EnvFilter {
    parse(value, DEFAULT_FILE_FILTER)
}

fn enabled(filter: &EnvFilter) -> bool {
    filter.max_level_hint() != Some(LevelFilter::OFF)
}

impl LogArgs {
    /// Plugin to set on `DefaultPlugins`, or to add beside `MinimalPlugins`. `name` prefixes the
    /// log files, so the game and the viewer do not interleave.
    pub fn plugin(&self, app: &mut App, name: &'static str) -> LogPlugin {
        let sinks = Sinks {
            console: self.console.clone(),
            file: self.file.clone(),
            dir: self.dir.clone(),
            name,
        };
        // The plugin's own filter applies to both sinks, so it must admit the most verbose one.
        let level = [console_filter(&sinks.console), file_filter(&sinks.file)]
            .iter()
            .filter_map(EnvFilter::max_level_hint)
            .max()
            .and_then(LevelFilter::into_level)
            .unwrap_or(Level::ERROR);
        app.insert_resource(sinks);
        LogPlugin {
            level,
            filter: crate::log_filter(),
            custom_layer: file_layer,
            fmt_layer: console_layer,
        }
    }

    /// Records what a shared log needs first: version, platform, command line and log settings.
    /// Call right after the plugin is added.
    pub fn announce(&self, name: &str) {
        info!(
            "{name} {} on {}/{}, pid {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::process::id()
        );
        info!(
            "arguments {:?}, working directory {:?}",
            std::env::args().skip(1).collect::<Vec<_>>(),
            std::env::current_dir().unwrap_or_default()
        );
        let file = if enabled(&file_filter(&self.file)) {
            format!("`{}` in {}", self.file, self.dir.display())
        } else {
            "disabled".into()
        };
        info!("logging: console `{}`, file {file}", self.console);
    }
}

fn console_layer(app: &mut App) -> Option<BoxedFmtLayer> {
    let sinks = app.world().get_resource::<Sinks>()?;
    Some(
        fmt::layer()
            // Colour only a terminal, and honour https://no-color.org/.
            .with_ansi(std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none())
            .with_writer(std::io::stderr)
            .with_filter(console_filter(&sinks.console))
            .boxed(),
    )
}

fn file_layer(app: &mut App) -> Option<BoxedLayer> {
    let sinks = app.world().get_resource::<Sinks>()?;
    let filter = file_filter(&sinks.file);
    if !enabled(&filter) {
        return None;
    }
    file_sink(&sinks.dir, sinks.name, filter)
        .inspect_err(|error| {
            // The subscriber is not installed yet, so stderr is the only place to say so.
            eprintln!("log file unavailable, logging to the console only: {error:#}");
        })
        .ok()
}

fn file_sink(dir: &Path, name: &str, filter: EnvFilter) -> anyhow::Result<BoxedLayer> {
    // The appender prunes old files while opening and complains when the directory is missing.
    std::fs::create_dir_all(dir)
        .with_context(|| format!("{}: creating the log directory", dir.display()))?;
    let writer = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(name)
        .filename_suffix("log")
        .max_log_files(KEPT_FILES)
        .build(dir)
        .with_context(|| format!("{}: opening the log file", dir.display()))?;
    Ok(fmt::layer()
        .with_ansi(false)
        .with_writer(writer)
        .with_filter(filter)
        .boxed())
}

/// Logs a panic with its backtrace before the previous hook reports it, so a shared log shows
/// why the process ended. Install after the log plugin.
pub fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!(
            target: "panic",
            "{info}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::log::tracing::{self, subscriber::with_default};
    use bevy::log::tracing_subscriber::{Registry, layer::SubscriberExt};
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        log: LogArgs,
    }

    fn read_logs(dir: &Path) -> String {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect()
    }

    #[test]
    fn file_filter_is_independent_and_covers_every_cyrus_crate() {
        let dir = std::env::temp_dir().join(format!("cyrus-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let layer = file_sink(&dir, "test", file_filter("")).unwrap();
        with_default(Registry::default().with(layer), || {
            tracing::debug!(target: "cyrus_game::player", "game detail");
            tracing::debug!(target: "cyrus", "binary detail");
            tracing::debug!(target: "winit", "dependency noise");
            tracing::info!(target: "winit", "dependency notice");
        });
        let text = read_logs(&dir);
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            text.contains("game detail") && text.contains("binary detail"),
            "{text}"
        );
        assert!(
            text.contains("dependency notice") && !text.contains("dependency noise"),
            "{text}"
        );
        assert!(!text.contains('\u{1b}'), "no colour codes in the file");
        assert!(names.len() == 1 && names[0].starts_with("test.") && names[0].ends_with(".log"));
    }

    #[test]
    fn expected_dependency_noise_is_filtered_but_real_warnings_remain() {
        let dir = std::env::temp_dir().join(format!("cyrus-log-noise-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Bevy builds its filter as `{level},{filter}`.
        let filter = EnvFilter::new(format!("info,{}", crate::log_filter()));
        let layer = file_sink(&dir, "test", filter).unwrap();
        with_default(Registry::default().with(layer), || {
            tracing::warn!(target: "bevy_render::mesh::allocator", "empty mesh");
            tracing::info!(target: "rgpre::import::rgm::positioning", "section progress");
            tracing::warn!(target: "rgpre::import::rgm", "bytes remaining");
            tracing::warn!(target: "bevy_render::renderer", "other renderer warning");
        });
        let text = read_logs(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            !text.contains("empty mesh") && !text.contains("section progress"),
            "{text}"
        );
        assert!(
            text.contains("bytes remaining") && text.contains("other renderer warning"),
            "{text}"
        );
    }

    #[test]
    fn opening_a_file_deletes_all_but_the_newest_daily_files() {
        let dir = std::env::temp_dir().join(format!("cyrus-log-rotate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for day in 1..=10 {
            std::fs::write(dir.join(format!("test.2020-01-{day:02}.log")), "old").unwrap();
        }
        file_sink(&dir, "test", file_filter("")).unwrap();
        let mut kept: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        kept.sort();
        // Today's file counts toward the limit, so the oldest fake files go first.
        assert_eq!(kept.len(), KEPT_FILES, "{kept:?}");
        assert_eq!(kept[0], "test.2020-01-05.log", "{kept:?}");
        assert!(
            kept.last().unwrap() > &"test.2020-01-10.log".to_owned(),
            "today's file exists: {kept:?}"
        );
    }

    #[test]
    fn options_validate_filters_and_off_disables_the_file() {
        assert!(Cli::try_parse_from(["t", "--log=cyrus=loud"]).is_err());
        assert!(Cli::try_parse_from(["t", "--log-file=nope=loud"]).is_err());
        let cli =
            Cli::try_parse_from(["t", "--log=warn,cyrus_game=debug", "--log-file=off"]).unwrap();
        assert_eq!(cli.log.console, "warn,cyrus_game=debug");
        assert!(!enabled(&file_filter(&cli.log.file)));
        assert!(enabled(&file_filter("")));
        // A blank RUST_LOG falls back instead of silencing the console.
        assert_eq!(console_filter(" ").to_string(), "info");
    }

    #[test]
    fn plugin_admits_the_more_verbose_sink() {
        let mut app = App::new();
        let cli = Cli::try_parse_from(["t", "--log=warn", "--log-file=trace"]).unwrap();
        assert_eq!(cli.log.plugin(&mut app, "test").level, Level::TRACE);
        let cli = Cli::try_parse_from(["t", "--log=warn", "--log-file=off"]).unwrap();
        assert_eq!(cli.log.plugin(&mut app, "test").level, Level::WARN);
    }
}
