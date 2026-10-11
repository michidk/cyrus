use cyrus_game::fidelity::Area;
use cyrus_game::fidelity::Fidelity;
use cyrus_game::fidelity::Mode;

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

#[test]
fn environmental_modes_switch_independently_and_roundtrip() {
    let mut fidelity = Fidelity::default();
    for area in [Area::Water, Area::Fog, Area::Sky, Area::Lighting] {
        fidelity.set(area, Mode::Modern);
        let saved = toml::to_string(&fidelity).unwrap();
        let restored: Fidelity = toml::from_str(&saved).unwrap();
        for candidate in Area::ALL {
            assert_eq!(
                restored.mode(candidate),
                if candidate == area {
                    Mode::Modern
                } else {
                    Mode::Faithful
                }
            );
        }
        fidelity.set(area, Mode::Faithful);
    }
}
