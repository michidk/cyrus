//! Mod settings: declared as `[[setting]]` in a mod's `mod.toml`, edited in the Mods window,
//! saved to `mod-settings.json` in the game's config directory and read by mods through the `settings` hook.
use anyhow::{Context, ensure};
use bevy::ecs::prelude::Resource;
use bevy_egui::egui;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The player's setting values, in the game's config directory.
fn settings_file() -> std::path::PathBuf {
    cyrus_config::config_dir().join("mod-settings.json")
}

/// One setting a mod declares. `key` is what the mod reads; `label` is what the player sees.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub(crate) struct Setting {
    pub key: String,
    label: Option<String>,
    #[serde(flatten)]
    kind: Kind,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Kind {
    Bool {
        default: bool,
    },
    Number {
        default: f32,
        min: f32,
        max: f32,
    },
    Choice {
        default: String,
        options: Vec<String>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub(crate) enum Value {
    Bool(bool),
    Number(f32),
    Choice(String),
}

impl Setting {
    /// Rejects a declaration whose default or range could never be a valid value.
    pub fn validate(&self) -> anyhow::Result<()> {
        match &self.kind {
            Kind::Bool { .. } => {}
            Kind::Number { default, min, max } => ensure!(
                min <= max && (min..=max).contains(&default),
                "setting {}: number needs min <= default <= max",
                self.key
            ),
            Kind::Choice { default, options } => ensure!(
                options.contains(default),
                "setting {}: default {default:?} is not one of its options",
                self.key
            ),
        }
        Ok(())
    }

    fn default_value(&self) -> Value {
        match &self.kind {
            Kind::Bool { default } => Value::Bool(*default),
            Kind::Number { default, .. } => Value::Number(*default),
            Kind::Choice { default, .. } => Value::Choice(default.clone()),
        }
    }

    /// A saved value if it still fits this declaration, else the default.
    fn accept(&self, saved: Option<&Value>) -> Value {
        match (&self.kind, saved) {
            (Kind::Bool { .. }, Some(Value::Bool(value))) => Value::Bool(*value),
            (Kind::Number { min, max, .. }, Some(Value::Number(value))) if value.is_finite() => {
                Value::Number(value.clamp(*min, *max))
            }
            (Kind::Choice { options, .. }, Some(Value::Choice(value)))
                if options.contains(value) =>
            {
                Value::Choice(value.clone())
            }
            _ => self.default_value(),
        }
    }
}

/// Every mod's setting values by mod id, then key.
#[derive(Resource, Default)]
pub(crate) struct ModSettings {
    pub values: BTreeMap<String, BTreeMap<String, Value>>,
    dirty: bool,
}

impl ModSettings {
    /// Reads the saved values and fits them to the declarations in `mods`; values of mods that
    /// are not installed are kept for when they return.
    pub fn load<'a>(
        mods: impl IntoIterator<Item = (&'a str, &'a [Setting])>,
    ) -> anyhow::Result<Self> {
        let mut values: BTreeMap<String, BTreeMap<String, Value>> =
            match std::fs::read(settings_file()) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .with_context(|| settings_file().display().to_string())?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(error) => {
                    return Err(error).with_context(|| settings_file().display().to_string());
                }
            };
        for (id, declared) in mods {
            let saved = values.entry(id.to_owned()).or_default();
            for setting in declared {
                let value = setting.accept(saved.get(&setting.key));
                saved.insert(setting.key.clone(), value);
            }
        }
        Ok(Self {
            values,
            dirty: false,
        })
    }

    pub fn save(&mut self) -> anyhow::Result<()> {
        self.dirty = false;
        std::fs::write(settings_file(), serde_json::to_vec_pretty(&self.values)?)
            .with_context(|| settings_file().display().to_string())
    }

    /// Draws one mod's settings. Returns the save error, if saving failed this frame.
    pub fn edit(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        declared: &[Setting],
    ) -> Option<anyhow::Error> {
        let values = self.values.entry(id.to_owned()).or_default();
        for setting in declared {
            let Some(value) = values.get_mut(&setting.key) else {
                continue;
            };
            let label = setting.label.as_deref().unwrap_or(&setting.key);
            let changed = match (&setting.kind, value) {
                (Kind::Bool { .. }, Value::Bool(on)) => ui.checkbox(on, label).changed(),
                (Kind::Number { min, max, .. }, Value::Number(number)) => ui
                    .add(egui::Slider::new(number, *min..=*max).text(label))
                    .changed(),
                (Kind::Choice { options, .. }, Value::Choice(chosen)) => {
                    let mut changed = false;
                    egui::ComboBox::from_label(label)
                        .selected_text(chosen.as_str())
                        .show_ui(ui, |ui| {
                            for option in options {
                                changed |= ui
                                    .selectable_value(chosen, option.clone(), option)
                                    .changed();
                            }
                        });
                    changed
                }
                _ => false,
            };
            self.dirty |= changed;
        }
        // Dragging a slider changes the value every frame; write the file once it is let go.
        if self.dirty && !ui.input(|input| input.pointer.any_down()) {
            return self.save().err();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Vec<Setting> {
        #[derive(Deserialize)]
        struct Manifest {
            setting: Vec<Setting>,
        }
        toml::from_str::<Manifest>(source).unwrap().setting
    }

    const MANIFEST: &str = r#"
        [[setting]]
        key = "run"
        type = "bool"
        default = true
        [[setting]]
        key = "speed"
        label = "Speed"
        type = "number"
        default = 150
        min = 20
        max = 600.5
        [[setting]]
        key = "mode"
        type = "choice"
        default = "a"
        options = ["a", "b"]
    "#;

    #[test]
    fn a_manifest_declares_each_kind_of_setting() {
        let settings = parse(MANIFEST);
        assert_eq!(settings.len(), 3);
        assert!(settings.iter().all(|setting| setting.validate().is_ok()));
        assert_eq!(settings[1].label.as_deref(), Some("Speed"));
        assert_eq!(settings[1].default_value(), Value::Number(150.0));
    }

    #[test]
    fn invalid_declarations_are_rejected() {
        let bad = parse(
            r#"
            [[setting]]
            key = "speed"
            type = "number"
            default = 700
            min = 20
            max = 600
            [[setting]]
            key = "mode"
            type = "choice"
            default = "c"
            options = ["a", "b"]
        "#,
        );
        assert!(bad.iter().all(|setting| setting.validate().is_err()));
    }

    #[test]
    fn saved_values_are_clamped_or_fall_back_to_the_default() {
        let settings = parse(MANIFEST);
        let [run, speed, mode] = &settings[..] else {
            unreachable!()
        };
        assert_eq!(run.accept(Some(&Value::Bool(false))), Value::Bool(false));
        assert_eq!(
            speed.accept(Some(&Value::Number(9999.0))),
            Value::Number(600.5)
        );
        assert_eq!(
            speed.accept(Some(&Value::Number(f32::NAN))),
            Value::Number(150.0)
        );
        assert_eq!(speed.accept(Some(&Value::Bool(true))), Value::Number(150.0));
        assert_eq!(
            mode.accept(Some(&Value::Choice("b".into()))),
            Value::Choice("b".into())
        );
        assert_eq!(
            mode.accept(Some(&Value::Choice("z".into()))),
            Value::Choice("a".into())
        );
        assert_eq!(mode.accept(None), Value::Choice("a".into()));
    }
}
