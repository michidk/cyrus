//! Checks the portable player cache against the installed ISLAND terrain.
use cyrus_game::{install::GameInstall, player::data::PlayerScene};

#[test]
fn installed_scene_exports_original_player_and_ground_inputs() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let install = match GameInstall::open(&root) {
        Ok(install) => install,
        Err(error) => {
            eprintln!("skipped player export: no install ({error})");
            return;
        }
    };
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cyrus-config")
        .join(format!("player-export-test-{}", std::process::id()));
    let path = directory.join("ISLAND.glb");
    cyrus_game::scene::export_scene(&install, "ISLAND", &path).unwrap();
    let scene =
        PlayerScene::decode(&std::fs::read(path.with_extension("player")).unwrap()).unwrap();
    assert!(scene.unavailable.is_none(), "{:?}", scene.unavailable);
    assert_eq!(scene.settings.standing_height, -2);
    let player = scene.definition.unwrap();
    assert_eq!(player.forward_walk, 8);
    let support = scene
        .terrain
        .unwrap()
        .sample([10_448_896, -178_944, 10_502_144])
        .unwrap();
    assert_eq!(support.height, -680.0);
    assert_eq!(support.normal, [0, -256, 0]);
    assert_eq!(
        support.actor_height(player.positive_extent[1], scene.settings.standing_height),
        -190_464
    );
    std::fs::remove_dir_all(directory).unwrap();
}
