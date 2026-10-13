use cyrus_assets::sound::SurfaceSounds;

#[test]
fn cache_rejects_missing_rows_short_rows_and_invalid_effect_ids() {
    for value in [
        serde_json::json!([]),
        serde_json::json!(vec![vec![0u16; 256]; 11]),
        serde_json::json!(vec![vec![0u16; 255]; 12]),
        serde_json::json!(vec![vec![65536u32; 256]; 12]),
    ] {
        assert!(serde_json::from_value::<SurfaceSounds>(value).is_err());
    }
}

#[cfg(feature = "parsers")]
#[test]
fn remaps_survive_cache_roundtrip_with_unknown_bypass_and_checked_operands() {
    let parsed =
        SurfaceSounds::parse("[surfaces]\n[wood]\n200=1\n201=600\n[unknown]\n0=1").unwrap();
    let cached: SurfaceSounds =
        serde_json::from_slice(&serde_json::to_vec(&parsed).unwrap()).unwrap();
    assert_eq!(cached, parsed);
    assert_eq!(cached.animation_sound(7, 200), Some(1));
    assert_eq!(cached.animation_sound(7, 201), Some(600));
    assert_eq!(cached.animation_sound(0, 0), Some(0));
    assert_eq!(cached.animation_sound(8, 200), Some(200));
    for (surface, source) in [(12, 0), (255, 0), (0, -1), (7, -1), (7, 256)] {
        assert_eq!(cached.animation_sound(surface, source), None);
    }
    assert!(
        SurfaceSounds::parse("[surfaces]\n[wood]\n256=1")
            .unwrap_err()
            .contains("line 3")
    );
}
