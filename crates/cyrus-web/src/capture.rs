//! Opt-in world/camera observation and captured sky phase for matched browser captures.
use bevy::prelude::*;
use wasm_bindgen::JsValue;

pub fn register(app: &mut App) {
    let enabled = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|query| web_sys::UrlSearchParams::new_with_str(&query).ok())
        .is_some_and(|query| query.has("capture"));
    super::session::register(app, enabled);
    if enabled {
        app.add_systems(
            Update,
            (sync_sky, sync_texture).run_if(in_state(cyrus_game::AppState::InGame)),
        );
        app.add_systems(
            PostUpdate,
            observe.run_if(in_state(cyrus_game::AppState::InGame)),
        );
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SkyRequest {
    id: u32,
    world: String,
    direction: u16,
    offset: [f32; 2],
}

fn sync_sky(
    scene: Res<cyrus_world::lifecycle::SceneRequest>,
    prepared: Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
    ready: Option<Res<cyrus_render::ReadyScene>>,
    mut sky: ResMut<cyrus_render::environment::SkyScrollSync>,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let key = JsValue::from_str("__cyrusSkyScrollRequest");
    let result = (|| -> Result<(), JsValue> {
        let value = js_sys::Reflect::get(&window, &key)?;
        if value.is_null() || value.is_undefined() {
            return Ok(());
        }
        js_sys::Reflect::set(&window, &key, &JsValue::NULL)?;
        let mut id = None;
        let accepted = (|| -> anyhow::Result<()> {
            let text = value
                .as_string()
                .ok_or_else(|| anyhow::anyhow!("sky capture request must be a JSON string"))?;
            let request: SkyRequest = serde_json::from_str(&text)?;
            id = Some(request.id);
            anyhow::ensure!(
                request.world == scene.0,
                "sky capture world does not match the current world"
            );
            anyhow::ensure!(
                ready.as_ref().is_some_and(|ready| prepared
                    .as_ref()
                    .is_some_and(|world| ready.0 == world.animations)),
                "sky capture requires a ready rendered world"
            );
            sky.set(request.direction, request.offset)
        })();
        let response = serde_json::json!({"id": id, "accepted": accepted.is_ok(), "error": accepted.err().map(|error| error.to_string())});
        js_sys::Reflect::set(
            &window,
            &"__cyrusSkyScrollResult".into(),
            &js_sys::JSON::parse(&response.to_string())?,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        error!("browser sky capture request: {error:?}");
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TextureRequest {
    id: u32,
    world: String,
    image: String,
    frame: Option<usize>,
}

fn sync_texture(
    scene: Res<cyrus_world::lifecycle::SceneRequest>,
    prepared: Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
    ready: Option<Res<cyrus_render::ReadyScene>>,
    mut textures: ResMut<cyrus_render::fog::TextureAnimations>,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let result = (|| -> Result<(), JsValue> {
        let key = JsValue::from_str("__cyrusTextureFrameRequest");
        let value = js_sys::Reflect::get(&window, &key)?;
        if value.is_null() || value.is_undefined() {
            return Ok(());
        }
        js_sys::Reflect::set(&window, &key, &JsValue::NULL)?;
        let mut id = None;
        let accepted = (|| -> anyhow::Result<()> {
            let request: TextureRequest =
                serde_json::from_str(&value.as_string().ok_or_else(|| {
                    anyhow::anyhow!("texture capture request must be a JSON string")
                })?)?;
            id = Some(request.id);
            anyhow::ensure!(
                request.world == scene.0
                    && ready.as_ref().is_some_and(|ready| {
                        prepared
                            .as_ref()
                            .is_some_and(|world| ready.0 == world.animations)
                    }),
                "texture capture requires the requested ready world"
            );
            textures.hold(&request.image, request.frame)
        })();
        let response = serde_json::json!({"id": id, "accepted": accepted.is_ok(), "error": accepted.err().map(|error| error.to_string())});
        js_sys::Reflect::set(
            &window,
            &"__cyrusTextureFrameResult".into(),
            &js_sys::JSON::parse(&response.to_string())?,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        error!("browser texture capture request: {error:?}");
    }
}

fn observe(
    request: Res<cyrus_world::lifecycle::SceneRequest>,
    textures: Res<cyrus_render::fog::TextureAnimations>,
    prepared: Option<Res<cyrus_world::lifecycle::PreparedWorld>>,
    ready_scene: Option<Res<cyrus_render::ReadyScene>>,
    camera: Query<&Transform, With<cyrus_render::camera::ViewCamera>>,
) {
    let (Some(window), Ok(camera)) = (web_sys::window(), camera.single()) else {
        return;
    };
    let ready = ready_scene.as_ref().is_some_and(|ready| {
        prepared
            .as_ref()
            .is_some_and(|world| ready.0 == world.animations)
    });
    let record = js_sys::Object::new();
    let view = camera
        .translation
        .to_array()
        .into_iter()
        .chain(camera.rotation.to_array())
        .map(|value| JsValue::from_f64(f64::from(value)))
        .collect::<js_sys::Array>();
    let result = (|| {
        let frames = serde_json::to_string(&textures.snapshot())
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        js_sys::Reflect::set(&record, &"textures".into(), &js_sys::JSON::parse(&frames)?)?;
        js_sys::Reflect::set(&record, &"world".into(), &request.0.as_str().into())?;
        js_sys::Reflect::set(&record, &"view".into(), &view)?;
        js_sys::Reflect::set(&record, &"ready".into(), &ready.into())?;
        js_sys::Reflect::set(&window, &"__cyrusCapture".into(), &record)
    })();
    if let Err(error) = result {
        error!("browser capture observation: {error:?}");
    }
}
