use cyrus_game::fidelity::{Area, Fidelity, Mode};

#[test]
fn every_area_defaults_to_faithful() {
    let f = Fidelity::default();
    for area in Area::ALL {
        assert_eq!(f.mode(area), Mode::Faithful, "{area:?}");
    }
}

#[test]
fn settings_toml_overrides_one_area() {
    let f: Fidelity = toml::from_str("camera = \"modern\"").unwrap();
    assert_eq!(f.mode(Area::Camera), Mode::Modern);
    assert_eq!(f.mode(Area::PlayerController), Mode::Faithful);
}
