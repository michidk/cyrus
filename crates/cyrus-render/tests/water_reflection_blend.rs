//! Execute the production shader blend without a GPU or installed game.

fn blend(sky_weight: f32, confidence: f32, reflectance: f32) -> f32 {
    let shader = include_str!("../src/fog/modern_lighting/modern_post.wesl");
    let start = shader.find("fn reflection_blend(").unwrap();
    let end = start + shader[start..].find("\n}").unwrap() + 2;
    let shader = wesl::CompileResult {
        syntax: format!("@const {}", &shader[start..end]).parse().unwrap(),
        ..Default::default()
    };
    let expression = format!(
        "reflection_blend(vec3(0.2f), vec3(0.4f), {sky_weight:?}f, vec3(0.8f), {confidence:?}f, {reflectance:?}f).x"
    );
    let bytes = shader.eval(&expression).unwrap().to_buffer().unwrap();
    f32::from_le_bytes(bytes.try_into().unwrap())
}

#[test]
fn missing_screen_geometry_retains_the_surface_unless_sky_is_visible() {
    assert!((blend(0.0, 0.0, 0.5) - 0.2).abs() < 1e-6);
    assert!((blend(1.0, 0.0, 0.5) - 0.3).abs() < 1e-6);
    assert!((blend(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
    assert!((blend(1.0, 1.0, 0.0) - 0.2).abs() < 1e-6);
}

#[test]
fn confidence_blends_continuously_without_a_hit_miss_colour_switch() {
    let mut previous = blend(0.0, 0.0, 0.5);
    for step in 1..=100 {
        let value = blend(0.0, step as f32 / 100.0, 0.5);
        assert!((value - previous - 0.003).abs() < 1e-6);
        previous = value;
    }
    assert!((blend(-1.0, -1.0, 0.5) - 0.2).abs() < 1e-6);
    assert!((blend(2.0, 2.0, 0.5) - 0.5).abs() < 1e-6);
}
