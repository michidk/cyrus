//! Older player records preserve unknown water configuration instead of inventing a depth.
use cyrus_assets::player::MovementSettings;

#[test]
fn player_settings_preserve_missing_and_signed_swim_depth() {
    let mut record = serde_json::json!({"standing_height":-2,"post_collide_height":50,"slide_range":1,"turn_speed":12,"turn_max_speed":48,"poly_push_units":16});
    let old: MovementSettings = serde_json::from_value(record.clone()).unwrap();
    assert_eq!(old.swim_depth, None);
    for depth in [-12, 0, 7, 40] {
        record["swim_depth"] = depth.into();
        let settings: MovementSettings = serde_json::from_value(record.clone()).unwrap();
        assert_eq!(settings.swim_depth, Some(depth));
        assert_eq!(serde_json::to_value(settings).unwrap()["swim_depth"], depth);
    }
}
