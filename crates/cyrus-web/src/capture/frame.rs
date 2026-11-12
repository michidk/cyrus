//! Retained browser pixels and actor state from the requested render extraction.
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use serde_json::{Value, json};
use wasm_bindgen::JsValue;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    boundary: cyrus_control::FrameBoundary,
}

#[derive(Resource, Default)]
struct Capture {
    pending: Option<Request>,
    active: Option<Entity>,
    deadline: f64,
}

#[derive(Component)]
struct Frozen {
    id: u64,
    boundary: Value,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<Capture>().add_systems(Last, capture);
}

fn publish(value: Value) -> Result<JsValue, JsValue> {
    let result = js_sys::JSON::parse(&value.to_string())?;
    if let Some(window) = web_sys::window() {
        js_sys::Reflect::set(&window, &"__cyrusFrameCaptureResult".into(), &result)?;
    }
    Ok(result)
}

fn snapshot(world: &mut World) -> anyhow::Result<(Value, bool)> {
    anyhow::ensure!(
        world
            .get_resource::<State<cyrus_game::AppState>>()
            .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame),
        "browser frame requires an active world"
    );
    let actors = world
        .get_resource::<cyrus_script::script::ScriptActors>()
        .ok_or_else(|| anyhow::anyhow!("browser frame: actors unavailable"))?
        .current
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("browser frame: logical world unavailable"))?;
    let player = actors
        .selected_player()
        .ok_or_else(|| anyhow::anyhow!("browser frame: selected player unavailable"))?;
    let clock = world
        .get_resource::<cyrus_script::clock::SimulationClock>()
        .ok_or_else(|| anyhow::anyhow!("browser frame: clock unavailable"))?;
    let target = world
        .resource::<Capture>()
        .pending
        .as_ref()
        .unwrap()
        .boundary;
    let contact = match target {
        cyrus_control::FrameBoundary::WallContact { after_update, .. } => {
            actors.first_selected_wall_contact_after(after_update)?
        }
        _ => None,
    };
    let reached = target
        .reached(
            actors.generation(),
            clock.elapsed_frame_scale(),
            actors.ordinary_updates(),
            contact,
        )
        .map_err(anyhow::Error::msg)?;
    if !reached {
        return Ok((Value::Null, false));
    }
    let boundary = json!({"ready":true,"execution":"running","frame":actors.ordinary_updates(),
        "generation":actors.generation(),"update":actors.ordinary_updates(),
        "wall_contact_update":actors.selected_wall_contact_update(),
        "elapsed_scale":clock.elapsed_frame_scale(),"normal_frame_rate":clock.settings().normal_frame_rate,
        "game_seconds":clock.game_seconds(),"player":actors.inspect(player.index,1)["actors"][0],
        "render_frame":world.resource::<bevy::diagnostic::FrameCount>().0,
        "event":"render_frame","phase":"render_extraction"});
    let prepared = world
        .get_resource::<cyrus_world::lifecycle::PreparedWorld>()
        .ok_or_else(|| anyhow::anyhow!("browser frame: world not prepared"))?;
    anyhow::ensure!(
        world
            .get_resource::<cyrus_render::ReadyScene>()
            .is_some_and(|ready| ready.0 == prepared.animations),
        "browser frame: rendered world not ready"
    );
    let scene = prepared.requested_name.clone();
    anyhow::ensure!(
        world.query::<&Screenshot>().iter(world).next().is_none(),
        "another browser screenshot is pending"
    );
    anyhow::ensure!(
        world
            .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
            .iter(world)
            .count()
            == 1,
        "browser frame requires one primary window"
    );
    let mut boundary = boundary;
    boundary["scene"] = json!(scene);
    let presentation = world
        .query_filtered::<(&Transform, &GlobalTransform), With<cyrus_script::player::Player>>()
        .single(world)
        .map_err(|error| anyhow::anyhow!("browser frame player presentation: {error}"))?;
    boundary["presentation"] = json!({"translation":presentation.0.translation.to_array(),
        "rotation":presentation.0.rotation.to_array(),"global":presentation.1.to_matrix().to_cols_array()});
    Ok((boundary, reached))
}

fn capture(world: &mut World) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let result = (|| -> Result<(), JsValue> {
        let key = JsValue::from_str("__cyrusFrameCaptureRequest");
        let value = js_sys::Reflect::get(&window, &key)?;
        let now = world.resource::<Time<Real>>().elapsed_secs_f64();
        if !value.is_null() && !value.is_undefined() {
            js_sys::Reflect::set(&window, &key, &JsValue::NULL)?;
            let retained = js_sys::Reflect::get(&window, &"__cyrusFrameCaptureResult".into())?;
            let busy = world.resource::<Capture>().pending.is_some()
                || world.resource::<Capture>().active.is_some();
            if busy || (!retained.is_null() && !retained.is_undefined()) {
                js_sys::Reflect::set(
                    &window,
                    &"__cyrusFrameCaptureError".into(),
                    &"capture pending or result unread".into(),
                )?;
            } else {
                match value
                    .as_string()
                    .ok_or_else(|| "frame request must be a JSON string".to_owned())
                    .and_then(|text| {
                        serde_json::from_str::<Request>(&text).map_err(|error| error.to_string())
                    }) {
                    Ok(request) => {
                        let mut capture = world.resource_mut::<Capture>();
                        capture.pending = Some(request);
                        capture.deadline = now + 30.0;
                    }
                    Err(error) => {
                        publish(json!({"ok":false,"error":error}))?;
                    }
                }
            }
        }
        if world.resource::<Capture>().pending.is_none()
            && world.resource::<Capture>().active.is_none()
        {
            return Ok(());
        }
        if now >= world.resource::<Capture>().deadline {
            let mut capture = world.resource_mut::<Capture>();
            let id = capture.pending.take().map(|request| request.id);
            let entity = capture.active.take();
            let id = id.or_else(|| {
                entity.and_then(|entity| world.get::<Frozen>(entity).map(|frozen| frozen.id))
            });
            if let Some(entity) = entity {
                let _ = world.despawn(entity);
            }
            publish(json!({"id":id,"ok":false,"error":"browser frame capture deadline expired"}))?;
            return Ok(());
        }
        if world.resource::<Capture>().active.is_some() {
            return Ok(());
        }
        match snapshot(world) {
            Ok((_, false)) => {}
            Ok((boundary, true)) => {
                let request = world.resource_mut::<Capture>().pending.take().unwrap();
                let entity = world
                    .spawn((
                        Screenshot::primary_window(),
                        Frozen {
                            id: request.id,
                            boundary,
                        },
                    ))
                    .observe(retain)
                    .id();
                world.resource_mut::<Capture>().active = Some(entity);
            }
            Err(error) => {
                let request = world.resource_mut::<Capture>().pending.take().unwrap();
                publish(json!({"id":request.id,"ok":false,"error":error.to_string()}))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        error!("browser frame capture: {error:?}");
    }
}

fn retain(
    event: On<ScreenshotCaptured>,
    frozen: Query<&Frozen>,
    mut capture: ResMut<Capture>,
    mut commands: Commands,
) {
    let Ok(frozen) = frozen.get(event.entity) else {
        return;
    };
    let result = (|| -> Result<(), JsValue> {
        let Some(bytes) = event.image.data.as_ref() else {
            publish(json!({"id":frozen.id,"ok":false,"error":"browser screenshot has no pixels"}))?;
            return Ok(());
        };
        let result = js_sys::JSON::parse(
            &json!({"id":frozen.id,"ok":true,"boundary":frozen.boundary,
            "width":event.image.width(),"height":event.image.height(),
            "format":format!("{:?}",event.image.texture_descriptor.format)})
            .to_string(),
        )?;
        js_sys::Reflect::set(
            &result,
            &"pixels".into(),
            &js_sys::Uint8Array::from(bytes.as_slice()),
        )?;
        if let Some(window) = web_sys::window() {
            js_sys::Reflect::set(&window, &"__cyrusFrameCaptureResult".into(), &result)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        error!("browser retained frame: {error:?}");
    }
    capture.active = None;
    commands.entity(event.entity).despawn();
}
