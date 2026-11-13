#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
//! A mod's script patch against the installed ISLAND map. The patch list is fixed per process, so
//! this lives in its own test binary.
use cyrus_assets::script::Scripts;
use cyrus_assets::script_patch::{ScriptPatch, set_script_patches};
use cyrus_soup::Native;
use cyrus_soup::program::Operation;

fn calls(scripts: &Scripts, actor: &str, native: Native) -> usize {
    scripts.actors[actor]
        .program
        .steps
        .values()
        .filter(
            |step| matches!(&step.operation, Operation::Call { call } if call.function == native),
        )
        .count()
}

#[test]
fn a_patch_lets_the_cut_serpent_run_the_script_that_follows_its_endless_loop() {
    let Ok(install) = cyrus_assets::install::GameInstall::locate(None) else {
        eprintln!("skipped: no install (script patch)");
        return;
    };
    let root = install.root.clone();
    let map = rgpre::import::rgm::parse_rgm_file(
        &std::fs::read(root.join("maps").join("ISLAND.RGM")).unwrap(),
    )
    .unwrap();
    // ISLAND is world 1 in the installed WORLD.INI.
    let original = cyrus_assets::script_export::prepare(&map, &root, 1).unwrap();
    assert_eq!(
        calls(&original, "SERPENT", Native::BeginCombat),
        0,
        "the installed script ends in a loop before it can fight"
    );
    assert!(set_script_patches(vec![ScriptPatch {
        world: "island".into(),
        actor: "serpent".into(),
        remove: vec![],
        fall_through: vec![4],
    }]));
    let patched = cyrus_assets::script_export::prepare(&map, &root, 1).unwrap();
    patched.validate().unwrap();
    assert_eq!(calls(&patched, "SERPENT", Native::BeginCombat), 1);
    assert_eq!(
        calls(&patched, "SERPENT", Native::ShowMe),
        1,
        "it shows itself when the player is near"
    );
    // The script still hides it first, and only the named actor changes.
    assert_eq!(calls(&patched, "SERPENT", Native::HideMe), 2);
    assert_eq!(calls(&patched, "SERPDEAD", Native::ShowMe), 1);
    assert_ne!(original.content_id, patched.content_id);
}
