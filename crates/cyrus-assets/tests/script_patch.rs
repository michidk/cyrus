//! A mod's script patch against the installed ISLAND map. The patch list is fixed per process, so
//! this lives in its own test binary.
use cyrus_assets::script_patch::{ScriptPatch, set_script_patches};
use cyrus_soup::Native;
use cyrus_soup::program::Operation;

fn hides(scripts: &cyrus_assets::script::Scripts, actor: &str) -> usize {
    scripts.actors[actor]
        .program
        .steps
        .values()
        .filter(|step| matches!(&step.operation, Operation::Call { call } if call.function == Native::HideMe))
        .count()
}

#[test]
fn a_patch_makes_the_cut_serpent_stop_hiding_itself() {
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
        hides(&original, "SERPENT"),
        1,
        "the installed script hides it"
    );
    assert!(set_script_patches(vec![ScriptPatch {
        world: "island".into(),
        actor: "serpent".into(),
        remove: vec![Native::HideMe],
    }]));
    let patched = cyrus_assets::script_export::prepare(&map, &root, 1).unwrap();
    patched.validate().unwrap();
    assert_eq!(hides(&patched, "SERPENT"), 0);
    // Only the named actor changes: the dead serpent still starts hidden.
    assert_eq!(hides(&patched, "SERPDEAD"), hides(&original, "SERPDEAD"));
    assert_ne!(original.content_id, patched.content_id);
}
