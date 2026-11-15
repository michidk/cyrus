//! The cheats a player can switch on, as the game's `REDGUARD.CHT` holds them.
//!
//! The file is the engine's 64-slot cheat state; preservation owns its format and the cheat names. The
//! game reads the copy in the config directory before the one in the install and never writes the
//! install, so the launcher does the same: it starts from whichever file the game would read and writes
//! the config directory's copy. The three startup slots the game takes from `SYSTEM.INI` (`task`,
//! `animation`, `nodemarker`), the two commands (`oracle`, `savecheats`) and `oddjob`, which no gameplay
//! check reads, are kept as they are and not offered.
use anyhow::Context;
use rgpre::import::cht::{
    CHEAT_NAMES, CHT_FILE_SIZE, CHT_SLOT_COUNT, encode_cht_file, parse_cht_file,
};
use std::path::{Path, PathBuf};

/// The cheat file the launcher and the game write, in the config directory.
pub fn local_file() -> PathBuf {
    cyrus_config::config_dir().join("REDGUARD.CHT")
}

/// The cheats with a gameplay effect, in the order the window lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cheat {
    MagicCarpet,
    DrEvil,
    NeverSayDie,
    GoldFinger,
    MoonRaker,
    DrNo,
    YeahBaby,
}

impl Cheat {
    pub const ALL: [Cheat; 7] = [
        Cheat::MagicCarpet,
        Cheat::DrEvil,
        Cheat::NeverSayDie,
        Cheat::GoldFinger,
        Cheat::MoonRaker,
        Cheat::DrNo,
        Cheat::YeahBaby,
    ];

    /// The name the game knows the cheat by.
    pub fn name(self) -> &'static str {
        match self {
            Cheat::MagicCarpet => "magiccarpet",
            Cheat::DrEvil => "drevil",
            Cheat::NeverSayDie => "neversaydie",
            Cheat::GoldFinger => "goldfinger",
            Cheat::MoonRaker => "moonraker",
            Cheat::DrNo => "drno",
            Cheat::YeahBaby => "yeahbaby",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Cheat::MagicCarpet => "Fly",
            Cheat::DrEvil => "Cannot die",
            Cheat::NeverSayDie => "Player always far away",
            Cheat::GoldFinger => "Torch never burns out",
            Cheat::MoonRaker => "No player collision",
            Cheat::DrNo => "Freeze other characters",
            Cheat::YeahBaby => "Move through surfaces",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Cheat::MagicCarpet => "No gravity or falling: the player walks through the air.",
            Cheat::DrEvil => "The player takes no damage and never dies.",
            Cheat::NeverSayDie => {
                "Scripts that measure the player's distance see a very large one."
            }
            Cheat::GoldFinger => "A burning torch stays lit.",
            Cheat::MoonRaker => "The player skips obstacle and character overlap collision.",
            Cheat::DrNo => "Skips most character updates other than the player's.",
            Cheat::YeahBaby => "Page Up and Page Down move up and down through surfaces.",
        }
    }

    fn slot(self) -> usize {
        CHEAT_NAMES
            .iter()
            .position(|name| *name == self.name())
            .expect("every cheat is a name the engine knows")
    }

    /// Whether a stored raw value switches this cheat on. The engine's own checks differ: fly mode
    /// needs exactly one, the others any nonzero value.
    fn is_active(self, value: u32) -> bool {
        match self {
            Cheat::MagicCarpet => value == 1,
            _ => value != 0,
        }
    }
}

/// The 64 cheat slots as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    slots: [u32; CHT_SLOT_COUNT],
}

impl Default for State {
    fn default() -> Self {
        Self {
            slots: [0; CHT_SLOT_COUNT],
        }
    }
}

impl State {
    pub fn is_on(&self, cheat: Cheat) -> bool {
        cheat.is_active(self.slots[cheat.slot()])
    }

    /// Stores one for on and zero for off, the values the game's own toggle uses.
    pub fn set(&mut self, cheat: Cheat, on: bool) {
        self.slots[cheat.slot()] = u32::from(on);
    }

    /// Whether any offered cheat is on.
    pub fn any_on(&self) -> bool {
        Cheat::ALL.iter().any(|cheat| self.is_on(*cheat))
    }

    /// Switches every offered cheat off. The slots the launcher does not offer are kept.
    pub fn clear(&mut self) {
        for cheat in Cheat::ALL {
            self.set(cheat, false);
        }
    }
}

fn read(file: &Path) -> anyhow::Result<Option<State>> {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| file.display().to_string()),
    };
    anyhow::ensure!(
        bytes.len() == CHT_FILE_SIZE,
        "{}: a cheat file is {CHT_FILE_SIZE} bytes, this one is {}",
        file.display(),
        bytes.len()
    );
    let parsed = parse_cht_file(&bytes).with_context(|| file.display().to_string())?;
    let mut state = State::default();
    for entry in parsed.named_cheats() {
        state.slots[entry.index] = entry.value;
    }
    for entry in parsed.nonzero_unnamed() {
        state.slots[entry.index] = entry.value;
    }
    Ok(Some(state))
}

/// The state the game starts from: `local` if it exists, else `install` (a game folder's own file),
/// else everything off.
pub fn load(local: &Path, install: Option<&Path>) -> anyhow::Result<State> {
    if let Some(state) = read(local)? {
        return Ok(state);
    }
    if let Some(folder) = install
        && let Some(state) = read(&folder.join("REDGUARD.CHT"))?
    {
        return Ok(state);
    }
    Ok(State::default())
}

/// Writes `state` to `local`.
pub fn save(local: &Path, state: &State) -> anyhow::Result<()> {
    if let Some(parent) = local.parent() {
        std::fs::create_dir_all(parent).with_context(|| parent.display().to_string())?;
    }
    std::fs::write(local, encode_cht_file(&state.slots))
        .with_context(|| format!("{}: saving cheats", local.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_with(path: &Path, set: &[(&str, u32)]) {
        let mut slots = [0u32; CHT_SLOT_COUNT];
        for (name, value) in set {
            slots[CHEAT_NAMES.iter().position(|n| n == name).unwrap()] = *value;
        }
        std::fs::write(path, encode_cht_file(&slots)).unwrap();
    }

    #[test]
    fn every_offered_cheat_is_a_name_the_engine_knows_once() {
        let mut slots: Vec<_> = Cheat::ALL.iter().map(|c| c.slot()).collect();
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), Cheat::ALL.len());
    }

    #[test]
    fn missing_files_are_everything_off() {
        let dir = tempfile::tempdir().unwrap();
        let state = load(&dir.path().join("none.CHT"), Some(dir.path())).unwrap();
        assert!(!state.any_on());
    }

    #[test]
    fn the_local_file_wins_over_the_installs() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("install");
        std::fs::create_dir(&install).unwrap();
        file_with(&install.join("REDGUARD.CHT"), &[("magiccarpet", 1)]);
        assert!(
            load(&dir.path().join("none"), Some(&install))
                .unwrap()
                .is_on(Cheat::MagicCarpet)
        );
        let local = dir.path().join("local.CHT");
        file_with(&local, &[("drevil", 1)]);
        let state = load(&local, Some(&install)).unwrap();
        assert!(state.is_on(Cheat::DrEvil) && !state.is_on(Cheat::MagicCarpet));
    }

    #[test]
    fn saving_turns_cheats_off_and_keeps_the_slots_not_offered() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("REDGUARD.CHT");
        file_with(
            &local,
            &[
                ("task", 1),
                ("animation", 1),
                ("magiccarpet", 1),
                ("drno", 1),
            ],
        );
        let mut state = load(&local, None).unwrap();
        assert!(state.any_on());
        state.clear();
        save(&local, &state).unwrap();
        let after = load(&local, None).unwrap();
        assert!(!after.any_on());
        let kept = |name| after.slots[CHEAT_NAMES.iter().position(|n| *n == name).unwrap()];
        assert_eq!(
            (kept("task"), kept("animation")),
            (1, 1),
            "engine switches are not cheats"
        );
    }

    #[test]
    fn fly_mode_needs_exactly_one_and_the_others_any_nonzero_value() {
        let mut state = State::default();
        state.slots[Cheat::MagicCarpet.slot()] = 2;
        state.slots[Cheat::DrEvil.slot()] = 2;
        assert!(!state.is_on(Cheat::MagicCarpet));
        assert!(state.is_on(Cheat::DrEvil));
    }

    #[test]
    fn a_file_of_the_wrong_size_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("REDGUARD.CHT");
        std::fs::write(&local, [0u8; 10]).unwrap();
        let error = format!("{:#}", load(&local, None).unwrap_err());
        assert!(
            error.contains("REDGUARD.CHT") && error.contains("256"),
            "{error}"
        );
    }
}
