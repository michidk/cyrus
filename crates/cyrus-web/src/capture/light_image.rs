//! One-shot browser acquisition of the actual material-bound CPU light image.
use bevy::prelude::*;
use wasm_bindgen::JsValue;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u32,
    name: String,
    primitive: usize,
    world: u32,
    generation: u64,
}

/// Registered only in capture mode, but consumes requests outside InGame too.
pub(super) fn respond(world: &mut World) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let result = (|| -> Result<(), JsValue> {
        let key = JsValue::from_str("__cyrusBoundLightImageRequest");
        let value = js_sys::Reflect::get(&window, &key)?;
        if value.is_null() || value.is_undefined() {
            return Ok(());
        }
        js_sys::Reflect::set(&window, &key, &JsValue::NULL)?;
        let mut id = None;
        let observation = (|| -> anyhow::Result<serde_json::Value> {
            let text = value
                .as_string()
                .ok_or_else(|| anyhow::anyhow!("bound light request must be a JSON string"))?;
            let request: Request = serde_json::from_str(&text)?;
            id = Some(request.id);
            anyhow::ensure!(
                world
                    .get_resource::<State<cyrus_game::AppState>>()
                    .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame),
                "bound light observation requires an active world"
            );
            let current = world
                .get_resource::<cyrus_game::script::ScriptActors>()
                .and_then(|actors| actors.current.as_ref())
                .ok_or_else(|| anyhow::anyhow!("bound light observation: logical world absent"))?;
            anyhow::ensure!(
                current.world == request.world && current.generation() == request.generation,
                "bound light request world/generation does not match current owner"
            );
            let image = cyrus_render::fog::BoundSceneLightImage::capture_named_primitive(
                world,
                &request.name,
                request.primitive,
            )?;
            let frame = world
                .get_resource::<bevy::diagnostic::FrameCount>()
                .ok_or_else(|| anyhow::anyhow!("bound light observation: frame count absent"))?
                .0;
            Ok(
                serde_json::json!({"frame":frame,"name":request.name,"primitive":request.primitive,"bound_light_image":image}),
            )
        })();
        let response = match observation {
            Ok(value) => serde_json::json!({"id":id,"ok":true,"result":value}),
            Err(error) => serde_json::json!({"id":id,"ok":false,"error":format!("{error:#}")}),
        };
        js_sys::Reflect::set(
            &window,
            &"__cyrusBoundLightImageResult".into(),
            &js_sys::JSON::parse(&response.to_string())?,
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        error!("browser bound light observation: {error:?}");
    }
}
