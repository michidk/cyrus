//! The launcher: what a player chooses before the game starts, and the command it starts it with.
//!
//! The launcher never links the game. It reads and writes the same small files the game reads at
//! startup (`mods.json`, the `[fidelity]` table of `settings.toml`, `install.json`) and starts the
//! `cyrus` executable beside it. Everything here is plain data and file access so that it can be
//! tested without a window; the window is the `cyrus-launcher` binary.
pub mod artwork;
pub mod cheats;
pub mod fidelity;
pub mod install;
pub mod launch;
pub mod lettering;
pub mod mods;
pub mod settings;
