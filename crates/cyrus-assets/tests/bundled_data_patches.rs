#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! The `[[ini-patch]]` and `[[text-patch]]` entries of the bundled mods against the installed
//! files: every section, key and line fragment they name is there, so no patch is silently dead.
use cyrus_assets::ini_patch::{IniPatch, apply_all as apply_ini};
use cyrus_assets::text_patch::{TextPatch, apply_all as apply_text};
use rgpre::import::rtx::{RtxEntry, parse_rtx_file};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Default)]
struct Manifest {
    #[serde(default, rename = "ini-patch")]
    ini_patches: Vec<IniPatch>,
    #[serde(default, rename = "text-patch")]
    text_patches: Vec<TextPatch>,
}

fn bundled(id: &str) -> Manifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../mods")
        .join(id)
        .join("mod.toml");
    toml::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

#[test]
fn the_bundled_ini_patches_apply_to_the_installed_files() {
    let Ok(install) = cyrus_assets::install::GameInstall::locate(None) else {
        eprintln!("skipped: no install (bundled ini patches)");
        return;
    };
    for id in ["one-punch", "unofficial-patch"] {
        let patches = bundled(id).ini_patches;
        assert!(!patches.is_empty(), "{id} has no ini patches");
        let mut files: Vec<&str> = patches.iter().map(|patch| patch.file.as_str()).collect();
        files.sort_unstable();
        files.dedup();
        for file in files {
            let text = std::fs::read_to_string(install.root.join(file)).unwrap();
            let patched = apply_ini(&patches, file, &text)
                .unwrap_or_else(|e| panic!("{id}: {e:#}"))
                .expect("the file has patches");
            assert_ne!(patched, text, "{id}: {file} did not change");
        }
    }
}

#[test]
fn one_punch_makes_every_sword_attack_of_cyrus_deal_one_hundred() {
    let Ok(install) = cyrus_assets::install::GameInstall::locate(None) else {
        eprintln!("skipped: no install (one punch)");
        return;
    };
    let text = std::fs::read_to_string(install.root.join("COMBAT.INI")).unwrap();
    let patched = apply_ini(&bundled("one-punch").ini_patches, "COMBAT.INI", &text)
        .unwrap()
        .unwrap();
    let damage = |text: &str, index: u32| {
        rgpre::import::combat_ini::CombatIni::parse(text)
            .unwrap()
            .attacks
            .get(&index)
            .and_then(|attack| attack.damage)
    };
    for index in 65..=71 {
        assert_eq!(damage(&patched, index), Some(100), "attack{index}");
    }
    assert_ne!(damage(&text, 65), Some(100), "the install is unpatched");
}

#[test]
fn the_unofficial_patch_text_corrections_apply_to_the_installed_dialogue() {
    let Ok(install) = cyrus_assets::install::GameInstall::locate(None) else {
        eprintln!("skipped: no install (unofficial patch text)");
        return;
    };
    let patches = bundled("unofficial-patch").text_patches;
    assert!(patches.len() > 80, "{} text patches", patches.len());
    let system = std::fs::read_to_string(install.root.join("SYSTEM.INI")).unwrap();
    let name = cyrus_assets::sound::SoundSettings::parse(&system).rtx_filename;
    let rtx = std::fs::read(install.root.join(name)).unwrap();
    let mut labels = std::collections::BTreeSet::new();
    for entry in parse_rtx_file(&rtx).unwrap().entries {
        let tag = entry.tag_str();
        let (RtxEntry::Text { mut text, .. }
        | RtxEntry::Audio {
            label: mut text, ..
        }) = entry;
        let label: [u8; 4] = tag.as_bytes().try_into().unwrap();
        if patches
            .iter()
            .any(|patch| patch.label.eq_ignore_ascii_case(&tag))
        {
            apply_text(
                &patches,
                label.map(|byte| byte.to_ascii_lowercase()),
                &mut text,
            )
            .unwrap_or_else(|e| panic!("{e:#}"));
            labels.insert(tag.to_ascii_lowercase());
        }
    }
    let named: std::collections::BTreeSet<String> = patches
        .iter()
        .map(|patch| patch.label.to_ascii_lowercase())
        .collect();
    assert_eq!(labels, named, "every patched label is in the dialogue file");
}
