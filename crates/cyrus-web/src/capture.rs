//! Opt-in, read-only world/camera observation for matched browser captures.
use bevy::prelude::*;
use wasm_bindgen::JsValue;

pub fn register(app: &mut App) {
    let enabled = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|query| web_sys::UrlSearchParams::new_with_str(&query).ok())
        .is_some_and(|query| query.has("capture"));
    if enabled {
        app.add_systems(
            PostUpdate,
            observe.run_if(in_state(cyrus_game::AppState::InGame)),
        );
    }
}

fn observe(
    request: Res<cyrus_world::lifecycle::SceneRequest>,
    camera: Query<&Transform, With<cyrus_render::camera::ViewCamera>>,
) {
    let (Some(window), Ok(camera)) = (web_sys::window(), camera.single()) else {
        return;
    };
    let record = js_sys::Object::new();
    let view = camera
        .translation
        .to_array()
        .into_iter()
        .chain(camera.rotation.to_array())
        .map(|value| JsValue::from_f64(f64::from(value)))
        .collect::<js_sys::Array>();
    let result = (|| {
        js_sys::Reflect::set(&record, &"world".into(), &request.0.as_str().into())?;
        js_sys::Reflect::set(&record, &"view".into(), &view)?;
        js_sys::Reflect::set(&window, &"__cyrusCapture".into(), &record)
    })();
    if let Err(error) = result {
        error!("browser capture observation: {error:?}");
    }
}
