//! Installed items open the examine view: each item with an inventory model has exported extents
//! the viewer rules accept, and its description labels follow the item's range.
use cyrus_game::preferences::Preferences;
use cyrus_script::item_viewer::{DescriptionLines, ItemViewer};
use cyrus_script::script::ScriptHud;

#[test]
fn every_installed_item_with_an_inventory_model_opens_the_view() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        return eprintln!("skipped: no install (examine view)");
    };
    let files = cyrus_assets::native::Files::open(&install).unwrap();
    let hud_files = cyrus_assets::game::prepare(&files, "hud/overlay.hud").unwrap();
    let asset: cyrus_assets::hud::HudAsset =
        serde_json::from_slice(&hud_files["hud/overlay.hud"]).unwrap();
    let hud = ScriptHud::default();
    hud.configure(&asset, &Preferences::default());
    let mut opened = 0;
    for &id in asset.inventory.keys() {
        let item = hud.examine_item(u32::from(id)).unwrap().unwrap();
        assert!(item.exported, "item {id}");
        let Some(model) = &item.model else {
            continue;
        };
        let extents = item
            .extents
            .unwrap_or_else(|| panic!("item {id} model {model}"));
        ItemViewer::open(extents, [0, 0], 0.75);
        let lines: Vec<_> =
            DescriptionLines::new(i32::from(id), item.description.as_deref(), 0).collect();
        match id {
            20 | 22 | 24 | 26 => assert!(lines.ends_with(&[*b"cip1"])),
            36 => assert_eq!(lines, [*b"cibe", *b"cibl"]),
            _ => {}
        }
        opened += 1;
    }
    assert!(opened > 20, "only {opened} items open the view");
    assert!(hud.examine_item(9999).unwrap().is_none());
}
