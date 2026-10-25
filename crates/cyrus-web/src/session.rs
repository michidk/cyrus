//! Browser book and diagnostic save/load with one outstanding host operation.
use anyhow::{Context, Result, ensure};
use bevy::{
    log::error,
    prelude::{App, AssetServer, Mut, PreUpdate, Resource, World},
};
use cyrus_session::{RuntimeSessionSnapshot, SessionBoundary, SessionBoundaryResult};
use serde_json::{Value, json};
use std::sync::{Mutex, mpsc};
use wasm_bindgen::JsValue;

#[path = "session/load_assets.rs"]
mod load_assets;
#[path = "session/document.rs"]
mod session_storage;
#[path = "session/storage.rs"]
mod storage;
use storage::SlotRequest;
#[path = "session/selection.rs"]
mod selection;
use selection::retain_selection;

const DATABASE: &str = "cyrus-sessions";

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Capture { id: u32 },
    Save { id: u32, key: String },
    Load { id: u32, key: String },
    Slots { id: u32, operation: SlotRequest },
    Result { id: u32 },
}

impl Request {
    fn id(&self) -> u32 {
        match self {
            Self::Capture { id }
            | Self::Save { id, .. }
            | Self::Load { id, .. }
            | Self::Slots { id, .. }
            | Self::Result { id } => *id,
        }
    }
}

enum IoResult {
    Read(Box<RuntimeSessionSnapshot>),
    SlotRead(Value),
    Planned(SlotRequest, Value),
    Written,
    Response(Value),
}
enum SaveTarget {
    Named(String),
    Slot(SlotRequest, u16),
}
type Io = Mutex<mpsc::Receiver<Result<IoResult>>>;
enum Phase {
    Capture(Option<SaveTarget>),
    Io(Io),
    Prepare(Box<RuntimeSessionSnapshot>, Box<load_assets::LoadAssets>),
    Commit,
}
#[derive(Resource, Default)]
struct Host {
    phase: Option<Phase>,
    result: Option<Result<Value>>,
    last_game: bool,
    book: bool,
    diagnostics: bool,
    autosave: bool,
}

pub fn register(app: &mut App, diagnostics: bool) {
    app.insert_resource(Host {
        diagnostics,
        ..Default::default()
    })
    .init_resource::<cyrus_startup::LastGameLoader>()
    .init_resource::<cyrus_startup::SaveLoadHost>()
    .add_systems(PreUpdate, update);
}

fn task(future: impl Future<Output = Result<IoResult>> + 'static) -> Io {
    let (sender, receiver) = mpsc::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = sender.send(future.await);
    });
    Mutex::new(receiver)
}

impl Host {
    fn request(&mut self, request: Request, world: &mut World) -> Result<Value> {
        ensure!(
            !self.last_game && !self.book,
            "book restore owns the browser session host"
        );
        if let Request::Result { id } = request {
            return match self.result.take() {
                Some(result) => result.map(|mut value| {
                    value["id"] = json!(id);
                    value
                }),
                None => Ok(json!({"id":id,"pending":self.phase.is_some()})),
            };
        }
        ensure!(
            self.phase.is_none() && self.result.is_none(),
            "browser session operation pending or result unread"
        );
        ensure!(
            world.resource::<SessionBoundary>().is_idle(),
            "session boundary pending or result unread"
        );
        let id = match request {
            Request::Capture { id } => {
                world.resource_mut::<SessionBoundary>().capture_live()?;
                self.phase = Some(Phase::Capture(None));
                id
            }
            Request::Save { id, key } => {
                ensure!(!key.is_empty(), "session storage key is empty");
                world.resource_mut::<SessionBoundary>().capture_live()?;
                self.phase = Some(Phase::Capture(Some(SaveTarget::Named(key))));
                id
            }
            Request::Load { id, key } => {
                ensure!(!key.is_empty(), "session storage key is empty");
                self.phase = Some(Phase::Io(task(async move {
                    let snapshot = session_storage::read(DATABASE, &key)
                        .await?
                        .with_context(|| format!("session {DATABASE}/{key} absent"))?;
                    Ok(IoResult::Read(Box::new(snapshot)))
                })));
                id
            }
            Request::Slots { id, operation } => {
                if matches!(
                    operation,
                    SlotRequest::Create { .. }
                        | SlotRequest::QuickSave
                        | SlotRequest::Overwrite { .. }
                ) {
                    self.phase = Some(Phase::Io(task(async move {
                        let plan = storage::plan(&operation).await?;
                        Ok(IoResult::Planned(operation, plan))
                    })));
                } else {
                    self.phase = Some(Phase::Io(task(async move {
                        let value = storage::slots(&operation, "").await?;
                        if matches!(operation, SlotRequest::Load { .. }) {
                            Ok(IoResult::SlotRead(value))
                        } else {
                            Ok(IoResult::Response(value))
                        }
                    })));
                }
                id
            }
            Request::Result { .. } => unreachable!(),
        };
        Ok(json!({"id":id,"pending":true}))
    }

    fn advance(&mut self, phase: Phase, world: &mut World) -> Result<Option<Phase>> {
        match phase {
            Phase::Capture(key) => {
                let Some(result) = world.resource_mut::<SessionBoundary>().take_result() else {
                    return Ok(Some(Phase::Capture(key)));
                };
                let SessionBoundaryResult::Captured(snapshot) = result? else {
                    anyhow::bail!("browser capture received a commit result");
                };
                if let Some(target) = key {
                    Ok(Some(Phase::Io(task(async move {
                        match target {
                            SaveTarget::Named(key) => {
                                session_storage::write(DATABASE, &key, &snapshot).await?;
                                Ok(IoResult::Written)
                            }
                            SaveTarget::Slot(operation, slot) => {
                                let document = String::from_utf8(snapshot.encode_document()?)?;
                                Ok(IoResult::Response(
                                    storage::publish(&operation, slot, &document).await?,
                                ))
                            }
                        }
                    }))))
                } else {
                    let bytes = snapshot.encode_document()?;
                    self.result = Some(Ok(
                        json!({"pending":false,"bytes":bytes.len(),"document":String::from_utf8(bytes)?}),
                    ));
                    Ok(None)
                }
            }
            Phase::Io(receiver) => {
                let received = receiver
                    .lock()
                    .map_err(|_| anyhow::anyhow!("session storage result lock poisoned"))?
                    .try_recv();
                match received {
                    Ok(result) => match result? {
                        IoResult::Planned(operation, value) => {
                            let slot = retain_selection(world, &value)?;
                            world.resource_mut::<SessionBoundary>().capture_live()?;
                            Ok(Some(Phase::Capture(Some(SaveTarget::Slot(
                                operation, slot,
                            )))))
                        }
                        IoResult::SlotRead(value) => {
                            retain_selection(world, &value)?;
                            let text = value["document"]
                                .as_str()
                                .context("slot load omitted its document")?;
                            let snapshot = RuntimeSessionSnapshot::decode_document(text.as_bytes())
                                .context("decoding browser save slot")?;
                            let assets = load_assets::LoadAssets::request(
                                &snapshot,
                                world.resource::<AssetServer>(),
                            )?;
                            Ok(Some(Phase::Prepare(Box::new(snapshot), Box::new(assets))))
                        }
                        IoResult::Response(mut value) => {
                            value["pending"] = json!(false);
                            self.result = Some(Ok(value));
                            Ok(None)
                        }
                        IoResult::Written => {
                            self.result = Some(Ok(json!({"pending":false,"stored":true})));
                            Ok(None)
                        }
                        IoResult::Read(snapshot) => {
                            let assets = load_assets::LoadAssets::request(
                                &snapshot,
                                world.resource::<AssetServer>(),
                            )?;
                            Ok(Some(Phase::Prepare(snapshot, Box::new(assets))))
                        }
                    },
                    Err(mpsc::TryRecvError::Empty) => Ok(Some(Phase::Io(receiver))),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        anyhow::bail!("session storage task ended without a result")
                    }
                }
            }
            Phase::Prepare(snapshot, mut assets) => {
                let Some(prepared) = assets.poll(&snapshot, world)? else {
                    return Ok(Some(Phase::Prepare(snapshot, assets)));
                };
                world
                    .resource_mut::<SessionBoundary>()
                    .load(*snapshot, prepared)?;
                Ok(Some(Phase::Commit))
            }
            Phase::Commit => {
                let Some(result) = world.resource_mut::<SessionBoundary>().take_result() else {
                    return Ok(Some(Phase::Commit));
                };
                ensure!(
                    matches!(result?, SessionBoundaryResult::Committed),
                    "browser load received a capture result"
                );
                self.result = Some(Ok(json!({"pending":false,"committed":true})));
                Ok(None)
            }
        }
    }
}

fn update(world: &mut World) {
    world.resource_scope(|world, mut host: Mut<Host>| {
        let quick_save = world
            .get_resource::<bevy::prelude::ButtonInput<bevy::prelude::KeyCode>>()
            .is_some_and(|keys| keys.just_pressed(bevy::prelude::KeyCode::F9));
        if quick_save
            && host.phase.is_none()
            && host.result.is_none()
            && world
                .get_resource::<bevy::prelude::State<cyrus_game::AppState>>()
                .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame)
            && world.resource::<SessionBoundary>().is_idle()
        {
            let operation = SlotRequest::QuickSave;
            host.phase = Some(Phase::Io(task(async move {
                let plan = storage::plan(&operation).await?;
                Ok(IoResult::Planned(operation, plan))
            })));
        }
        let autosave = world
            .get_resource::<bevy::prelude::State<cyrus_game::AppState>>()
            .is_some_and(|state| *state.get() == cyrus_game::AppState::Menu)
            && world.contains_resource::<cyrus_game::PausedGameplay>();
        if !autosave {
            host.autosave = false;
        }
        if autosave
            && !host.autosave
            && host.phase.is_none()
            && host.result.is_none()
            && world.resource::<SessionBoundary>().is_idle()
        {
            let operation = SlotRequest::AutoSave;
            host.phase = Some(Phase::Io(task(async move {
                let plan = storage::plan(&operation).await?;
                Ok(IoResult::Planned(operation, plan))
            })));
            host.autosave = true;
        }
        let request = world
            .resource_mut::<cyrus_startup::SaveLoadHost>()
            .take_request();
        if let Some(request) = request {
            let operation = match request {
                cyrus_startup::SaveLoadRequest::List => SlotRequest::List,
                cyrus_startup::SaveLoadRequest::Create { description } => {
                    SlotRequest::Create { description }
                }
                cyrus_startup::SaveLoadRequest::Overwrite {
                    slot,
                    revision,
                    description,
                } => SlotRequest::Overwrite {
                    slot,
                    revision,
                    description,
                },
                cyrus_startup::SaveLoadRequest::Load { slot, revision } => {
                    SlotRequest::Load { slot, revision }
                }
                cyrus_startup::SaveLoadRequest::Delete { slot, revision } => {
                    SlotRequest::Delete { slot, revision }
                }
            };
            match host.request(Request::Slots { id: 0, operation }, world) {
                Ok(_) => host.book = true,
                Err(error) => world
                    .resource_mut::<cyrus_startup::SaveLoadHost>()
                    .finish(Err(format!("{error:#}"))),
            }
        }

        if host.phase.is_none()
            && host.result.is_none()
            && world.resource::<SessionBoundary>().is_idle()
            && let Some(save) = world
                .resource_mut::<cyrus_startup::LastGameLoader>()
                .take_request()
        {
            host.last_game = true;
            host.phase = Some(Phase::Io(task(async move {
                let listing = storage::slots(&SlotRequest::List, "").await?;
                let slots = listing["slots"]
                    .as_array()
                    .context("save list omitted slots")?;
                let metadata = slots
                    .iter()
                    .find(|entry| entry["slot"].as_u64() == Some(u64::from(save.slot)))
                    .with_context(|| format!("retained save slot {} absent", save.slot))?;
                let revision = metadata["revision"]
                    .as_str()
                    .context("retained slot revision absent")?
                    .to_owned();
                let value = storage::slots(
                    &SlotRequest::Load {
                        slot: save.slot,
                        revision,
                    },
                    "",
                )
                .await?;
                let text = value["document"]
                    .as_str()
                    .context("retained slot document absent")?;
                let snapshot = RuntimeSessionSnapshot::decode_document(text.as_bytes())
                    .with_context(|| format!("decoding retained save slot {}", save.slot))?;
                Ok(IoResult::Read(Box::new(snapshot)))
            })));
        }
        if let Some(phase) = host.phase.take() {
            match host.advance(phase, world) {
                Ok(phase) => host.phase = phase,
                Err(error) => host.result = Some(Err(error)),
            }
        }
        if host.last_game
            && let Some(result) = host.result.take()
        {
            let completion = result.map(|_| ()).map_err(|error| format!("{error:#}"));
            if let Err(error) = world
                .resource_mut::<cyrus_startup::LastGameLoader>()
                .finish(completion)
            {
                error!("last-game completion rejected: {error:#}");
            }
            host.last_game = false;
        }
        if host.book
            && let Some(result) = host.result.take()
        {
            world
                .resource_mut::<cyrus_startup::SaveLoadHost>()
                .finish(result.map_err(|error| format!("{error:#}")));
            host.book = false;
        }
        if host.autosave
            && host.phase.is_none()
            && let Some(Err(error)) = host.result.take()
        {
            error!("Auto Save Game: {error:#}");
        }
        if !host.diagnostics {
            return;
        }
        let Some(window) = web_sys::window() else {
            return;
        };
        let operation = (|| -> Result<(), JsValue> {
            let key = JsValue::from_str("__cyrusSessionRequest");
            let value = js_sys::Reflect::get(&window, &key)?;
            if value.is_null() || value.is_undefined() {
                return Ok(());
            }
            js_sys::Reflect::set(&window, &key, &JsValue::NULL)?;
            let mut id = None;
            let result = (|| -> Result<Value> {
                let text = value
                    .as_string()
                    .context("session request must be a JSON string")?;
                let request: Request = serde_json::from_str(&text)?;
                id = Some(request.id());
                host.request(request, world)
            })();
            let response = match result {
                Ok(response) => response,
                Err(error) => json!({"id":id,"pending":false,"error":format!("{error:#}")}),
            };
            js_sys::Reflect::set(
                &window,
                &"__cyrusSessionResult".into(),
                &js_sys::JSON::parse(&response.to_string())?,
            )?;
            Ok(())
        })();
        if let Err(error) = operation {
            error!("browser session request: {error:?}");
        }
    });
}
