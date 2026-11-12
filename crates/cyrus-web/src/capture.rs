//! Opt-in world/camera observation and captured sky phase for matched browser captures.
mod audio;
mod frame;
mod light_image;

use bevy::prelude::*;
use wasm_bindgen::JsValue;

pub fn register(app: &mut App) {
    let enabled = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .and_then(|query| web_sys::UrlSearchParams::new_with_str(&query).ok())
        .is_some_and(|query| query.has("capture"));
    super::session::register(app, enabled);
    if enabled {
        audio::register(app);
        frame::register(app);
        app.add_systems(PostUpdate, (light_image::respond, publish_startup));
        app.add_systems(
            Update,
            (sync_sky, sync_texture, sync_acceptance)
                .run_if(in_state(cyrus_game::AppState::InGame)),
        );
        app.add_systems(
            PostUpdate,
            observe.run_if(in_state(cyrus_game::AppState::InGame)),
        );
        app.add_systems(
            PostUpdate,
            publish_menu.run_if(in_state(cyrus_game::AppState::Menu)),
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

/// Observe each startup phase, including movie-to-world handoff, without advancing its clock.
fn publish_startup(
    state: Res<State<cyrus_game::AppState>>,
    presentation: Res<cyrus_startup::StartupPresentation>,
    loading: Option<Res<cyrus_startup::StartGame>>,
    clock: Option<Res<cyrus_script::clock::SimulationClock>>,
    fades: Query<&BackgroundColor, With<cyrus_startup::WorldEntryFade>>,
    camera: Res<cyrus_script::script::CameraFrameState>,
    console: Option<Res<cyrus_console::Console>>,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let record = serde_json::json!({
        "state": format!("{:?}", state.get()),
        "console_open": console.as_ref().is_some_and(|console| console.open),
        "loading_world": loading.as_ref().and_then(|loading| loading.artwork_world()),
        "presentation": &*presentation,
        "world_fades": fades.iter().map(|color| color.0.alpha()).collect::<Vec<_>>(),
        "camera_published": camera.published.is_some(),
        "camera_pose": camera.published.as_ref().map(|camera| serde_json::json!({
            "position": camera.pose.position,
            "angles": camera.pose.angles,
            "render_position": camera.render.position,
            "view_angles": camera.view_angles,
        })),
        "clock": clock.as_ref().map(|clock| serde_json::json!({"game_seconds": clock.game_seconds()})),
    });
    let result = js_sys::JSON::parse(&record.to_string())
        .and_then(|value| js_sys::Reflect::set(&window, &"__cyrusStartup".into(), &value));
    if let Err(error) = result {
        error!("browser startup observation: {error:?}");
    }
}

/// The book menu's state (page, selected entry, entries, running movie) on `window.__cyrusMenu`,
/// like the native `state` command's `menu`, so a browser driver can see where the selection is
/// before it sends keys. Only while the app is in its menu state.
fn publish_menu(menu: Option<Res<cyrus_startup::MenuState>>) {
    let (Some(window), Some(menu)) = (web_sys::window(), menu) else {
        return;
    };
    let result = (|| -> Result<(), JsValue> {
        let text =
            serde_json::to_string(&*menu).map_err(|error| JsValue::from_str(&error.to_string()))?;
        js_sys::Reflect::set(&window, &"__cyrusMenu".into(), &js_sys::JSON::parse(&text)?)
            .map(|_| ())
    })();
    if let Err(error) = result {
        error!("browser menu observation: {error:?}");
    }
}

/// One acceptance-run action on the prepared script world, the browser counterpart of the native
/// control port's `place_player`, `mark_dead` and item commands. Positions are map units.
#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum AcceptanceRequest {
    ResetCamera {
        id: u32,
    },
    LoadScene {
        id: u32,
        name: String,
    },
    PlacePlayer {
        id: u32,
        x: i32,
        y: i32,
        z: i32,
        #[serde(default)]
        heading: u16,
    },
    MarkDead {
        id: u32,
        index: usize,
    },
    AddItem {
        id: u32,
        item: u16,
    },
}

fn sync_acceptance(
    mut scripts: Option<ResMut<cyrus_script::script::ScriptActors>>,
    hud: Option<Res<cyrus_script::script::ScriptHud>>,
    mut scene: ResMut<cyrus_world::lifecycle::SceneRequest>,
    mut commands: Commands,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let key = JsValue::from_str("__cyrusAcceptanceRequest");
    let result = (|| -> Result<(), JsValue> {
        let value = js_sys::Reflect::get(&window, &key)?;
        if value.is_null() || value.is_undefined() {
            return Ok(());
        }
        js_sys::Reflect::set(&window, &key, &JsValue::NULL)?;
        let mut id = None;
        let applied = (|| -> anyhow::Result<()> {
            let text = value
                .as_string()
                .ok_or_else(|| anyhow::anyhow!("acceptance request must be a JSON string"))?;
            let request: AcceptanceRequest = serde_json::from_str(&text)?;
            let world = scripts
                .as_mut()
                .and_then(|actors| actors.current.as_mut())
                .ok_or_else(|| anyhow::anyhow!("script world is not prepared"))?;
            match request {
                AcceptanceRequest::ResetCamera { id: request_id } => {
                    id = Some(request_id);
                    world.globals_mut().request_camera_reset();
                    Ok(())
                }
                AcceptanceRequest::LoadScene {
                    id: request_id,
                    name,
                } => {
                    id = Some(request_id);
                    anyhow::ensure!(!name.is_empty(), "scene name is empty");
                    scene.0 = name;
                    commands.insert_resource(cyrus_world::lifecycle::ReloadScene);
                    Ok(())
                }
                AcceptanceRequest::PlacePlayer {
                    id: request_id,
                    x,
                    y,
                    z,
                    heading,
                } => {
                    id = Some(request_id);
                    world.place_selected_player(
                        [x, y, z].map(|value| value.wrapping_mul(256)),
                        heading,
                    )
                }
                AcceptanceRequest::MarkDead {
                    id: request_id,
                    index,
                } => {
                    id = Some(request_id);
                    world.mark_actor_dead(index)
                }
                AcceptanceRequest::AddItem {
                    id: request_id,
                    item,
                } => {
                    id = Some(request_id);
                    let hud = hud
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("inventory is not prepared"))?;
                    world.add_player_item(hud, item)
                }
            }
        })();
        let response = serde_json::json!({"id": id, "applied": applied.is_ok(), "error": applied.err().map(|error| format!("{error:#}"))});
        js_sys::Reflect::set(
            &window,
            &"__cyrusAcceptanceResult".into(),
            &js_sys::JSON::parse(&response.to_string())?,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        error!("browser acceptance request: {error:?}");
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
    scripts: Option<Res<cyrus_script::script::ScriptActors>>,
    script_hud: Option<Res<cyrus_script::script::ScriptHud>>,
    session: cyrus_session::SessionBoundaryInspection,
    clock: Option<Res<cyrus_script::clock::SimulationClock>>,
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
        if let Some(snapshot) = session.snapshot(
            js_sys::Reflect::get(&window, &"__cyrusInspectSession".into())?.as_bool() == Some(true),
        ) {
            let text = serde_json::to_string(&snapshot)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            js_sys::Reflect::set(&record, &"session".into(), &js_sys::JSON::parse(&text)?)?;
        }
        if let (Some(world), Some(clock)) = (
            scripts.as_ref().and_then(|actors| actors.current.as_ref()),
            clock.as_ref(),
        ) {
            let boundary = serde_json::json!({"generation":world.generation(),"update":world.ordinary_updates(),
                "elapsed_scale":clock.elapsed_frame_scale(),"normal_frame_rate":clock.settings().normal_frame_rate,
                "wall_contact_update":world.selected_wall_contact_update()});
            js_sys::Reflect::set(
                &record,
                &"boundary".into(),
                &js_sys::JSON::parse(&boundary.to_string())?,
            )?;
        }
        // At most 64 script actors, only while a driver sets `__cyrusInspectScripts`; the
        // browser has no control port, so acceptance drivers read this like the `scripts` command.
        if js_sys::Reflect::get(&window, &"__cyrusInspectScripts".into())?.is_truthy()
            && let Some(world) = scripts.as_ref().and_then(|actors| actors.current.as_ref())
        {
            let offset = js_sys::Reflect::get(&window, &"__cyrusInspectScriptsOffset".into())?
                .as_f64()
                .filter(|offset| {
                    offset.is_finite()
                        && *offset >= 0.0
                        && *offset <= f64::from(u32::MAX)
                        && offset.fract() == 0.0
                })
                .unwrap_or(0.0) as usize;
            let text = world.inspect(offset, 64).to_string();
            js_sys::Reflect::set(&record, &"scripts".into(), &js_sys::JSON::parse(&text)?)?;
        }
        // The script HUD (inventory counts, selected item) under the same opt-in, like `state`'s `script_hud`.
        if js_sys::Reflect::get(&window, &"__cyrusInspectScripts".into())?.is_truthy()
            && let Some(hud) = script_hud.as_ref()
        {
            js_sys::Reflect::set(
                &record,
                &"script_hud".into(),
                &js_sys::JSON::parse(&hud.inspect().to_string())?,
            )?;
        }
        js_sys::Reflect::set(&window, &"__cyrusCapture".into(), &record)
    })();
    if let Err(error) = result {
        error!("browser capture observation: {error:?}");
    }
}
