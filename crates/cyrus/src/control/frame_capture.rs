//! Bounded native screenshots whose clock and PCM delivery cursor belong to the extracted frame.
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use cyrus_control::Response;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    time::{Duration, Instant},
};

struct Request {
    id: u64,
    path: PathBuf,
    pcm_path: Option<PathBuf>,
    reply: Sender<Response>,
    deadline: Instant,
    boundary: Option<cyrus_control::FrameBoundary>,
}

struct Active {
    request: Request,
    entity: Entity,
    completed: Mutex<mpsc::Receiver<Result<Value, String>>>,
    writing: Arc<AtomicBool>,
    timed_out: bool,
}

/// One readback and encoder at a time. Delivery polling never supplies the frame timestamp.
#[derive(Resource, Default)]
pub(super) struct FrameCaptures {
    queued: Option<Request>,
    active: Option<Active>,
}

impl FrameCaptures {
    pub(super) fn arm(
        &mut self,
        id: u64,
        path: PathBuf,
        pcm_path: Option<PathBuf>,
        reply: Sender<Response>,
        boundary: Option<cyrus_control::FrameBoundary>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.queued.is_none() && self.active.is_none(),
            "gameplay frame capture already pending"
        );
        anyhow::ensure!(
            path.extension().is_some_and(|extension| extension == "png"),
            "gameplay frame capture requires a PNG path"
        );
        anyhow::ensure!(
            !path.exists(),
            "gameplay frame capture output already exists: {}",
            path.display()
        );
        self.queued = Some(Request {
            id,
            path,
            pcm_path,
            reply,
            deadline: Instant::now() + Duration::from_secs(30),
            boundary,
        });
        Ok(())
    }
}

#[derive(Component)]
struct FrozenFrame {
    boundary: Value,
    path: PathBuf,
    completed: Sender<Result<Value, String>>,
    writing: Arc<AtomicBool>,
}

fn boundary(world: &mut World, pcm_path: Option<&PathBuf>) -> anyhow::Result<Value> {
    anyhow::ensure!(
        world
            .get_resource::<State<cyrus_game::AppState>>()
            .is_some_and(|state| *state.get() == cyrus_game::AppState::InGame),
        "gameplay frame requires an active world"
    );
    let prepared = world
        .get_resource::<cyrus_world::lifecycle::PreparedWorld>()
        .ok_or_else(|| anyhow::anyhow!("gameplay frame: world not prepared"))?;
    anyhow::ensure!(
        world
            .get_resource::<cyrus_render::ReadyScene>()
            .is_some_and(|ready| ready.0 == prepared.animations),
        "gameplay frame requires the requested ready scene"
    );
    let scene = prepared.requested_name.clone();
    anyhow::ensure!(
        world
            .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
            .iter(world)
            .count()
            == 1,
        "gameplay frame requires one primary window"
    );
    anyhow::ensure!(
        world.query::<&Screenshot>().iter(world).next().is_none(),
        "another screenshot is pending"
    );
    let presentation = world
        .query_filtered::<(&Transform, &GlobalTransform), With<cyrus_script::player::Player>>()
        .single(world)
        .map(|(local, global)| {
            json!({
                "translation": local.translation.to_array(), "rotation": local.rotation.to_array(),
                "global": global.to_matrix().to_cols_array(),
            })
        })
        .map_err(|error| {
            anyhow::anyhow!("gameplay frame: player presentation unavailable: {error}")
        })?;
    let actors = world
        .get_resource::<cyrus_script::script::ScriptActors>()
        .and_then(|actors| actors.current.as_ref())
        .ok_or_else(|| anyhow::anyhow!("gameplay frame: selected world unavailable"))?;
    let player = actors
        .selected_player()
        .ok_or_else(|| anyhow::anyhow!("gameplay frame: selected player unavailable"))?;
    let clock = world
        .get_resource::<cyrus_script::clock::SimulationClock>()
        .ok_or_else(|| anyhow::anyhow!("gameplay frame: simulation clock unavailable"))?;
    let pcm_frames = pcm_path
        .map(|path| -> anyhow::Result<u64> {
            let metadata = std::fs::metadata(path).map_err(|error| {
                anyhow::anyhow!("gameplay frame PCM {}: {error}", path.display())
            })?;
            anyhow::ensure!(
                metadata.is_file(),
                "gameplay frame PCM is not a regular file: {}",
                path.display()
            );
            Ok(metadata.len() / 4)
        })
        .transpose()?;
    let frame = world
        .get_resource::<bevy::diagnostic::FrameCount>()
        .ok_or_else(|| anyhow::anyhow!("gameplay frame: render frame counter unavailable"))?
        .0;
    Ok(json!({
        "ready": true, "scene": scene, "execution": "running",
        "frame": actors.ordinary_updates(), "render_frame": frame,
        "presentation": presentation,
        "wall_contact_update": actors.selected_wall_contact_update(),
        "update": actors.ordinary_updates(), "generation": actors.generation(),
        "elapsed_scale": clock.elapsed_frame_scale(), "normal_frame_rate": clock.settings().normal_frame_rate,
        "game_seconds": clock.game_seconds(), "player": actors.inspect(player.index, 1)["actors"][0],
        "event": "render_frame", "phase": "render_extraction",
        "pcm_delivery_frames": pcm_frames, "exact_pcm_event_mapping": false,
    }))
}

/// Last runs after simulation, animation, camera and transform publication, immediately before extraction.
pub(super) fn capture(world: &mut World) {
    let finished = world
        .resource::<FrameCaptures>()
        .active
        .as_ref()
        .and_then(|active| active.completed.lock().unwrap().try_recv().ok());
    if let Some(result) = finished {
        let active = world.resource_mut::<FrameCaptures>().active.take().unwrap();
        if !active.timed_out {
            let response = if Instant::now() >= active.request.deadline {
                Response::err(active.request.id, "gameplay frame capture deadline expired")
            } else {
                match result {
                    Ok(result) => Response::ok(active.request.id, result),
                    Err(error) => Response::err(active.request.id, error),
                }
            };
            let _ = active.request.reply.send(response);
        }
        if world.get_entity(active.entity).is_ok() {
            let _ = world.despawn(active.entity);
        }
    }
    let expired = world
        .resource::<FrameCaptures>()
        .active
        .as_ref()
        .is_some_and(|active| !active.timed_out && Instant::now() >= active.request.deadline);
    if expired {
        let mut active = world.resource_mut::<FrameCaptures>().active.take().unwrap();
        let _ = active.request.reply.send(Response::err(
            active.request.id,
            "gameplay frame capture deadline expired",
        ));
        if active.writing.load(Ordering::Acquire) {
            // A blocked encoder must not admit another encoder every timeout period.
            active.timed_out = true;
            world.resource_mut::<FrameCaptures>().active = Some(active);
        } else if world.get_entity(active.entity).is_ok() {
            let _ = world.despawn(active.entity);
        }
    }
    let Some(request) = world.resource_mut::<FrameCaptures>().queued.take() else {
        return;
    };
    let scheduled = (|| -> anyhow::Result<bool> {
        anyhow::ensure!(
            Instant::now() < request.deadline,
            "gameplay frame capture deadline expired"
        );
        let Some(target) = request.boundary else {
            return Ok(true);
        };
        let actors = world
            .get_resource::<cyrus_script::script::ScriptActors>()
            .and_then(|actors| actors.current.as_ref())
            .ok_or_else(|| anyhow::anyhow!("scheduled frame: logical world unavailable"))?;
        let clock = world
            .get_resource::<cyrus_script::clock::SimulationClock>()
            .ok_or_else(|| anyhow::anyhow!("scheduled frame: clock unavailable"))?;
        let contact = match target {
            cyrus_control::FrameBoundary::WallContact { after_update, .. } => {
                actors.first_selected_wall_contact_after(after_update)?
            }
            _ => None,
        };
        target
            .reached(
                actors.generation(),
                clock.elapsed_frame_scale(),
                actors.ordinary_updates(),
                contact,
            )
            .map_err(anyhow::Error::msg)
    })();
    match scheduled {
        Ok(false) => {
            world.resource_mut::<FrameCaptures>().queued = Some(request);
            return;
        }
        Err(error) => {
            let _ = request
                .reply
                .send(Response::err(request.id, error.to_string()));
            return;
        }
        Ok(true) => {}
    }
    let result = boundary(world, request.pcm_path.as_ref());
    let snapshot = match result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = request
                .reply
                .send(Response::err(request.id, format!("{error:#}")));
            return;
        }
    };
    let (tx, rx) = mpsc::channel();
    let writing = Arc::new(AtomicBool::new(false));
    let entity = world
        .spawn((
            Screenshot::primary_window(),
            FrozenFrame {
                boundary: snapshot,
                path: request.path.clone(),
                completed: tx,
                writing: writing.clone(),
            },
        ))
        .observe(write_frame)
        .id();
    world.resource_mut::<FrameCaptures>().active = Some(Active {
        request,
        entity,
        completed: Mutex::new(rx),
        writing,
        timed_out: false,
    });
}

fn write_frame(event: On<ScreenshotCaptured>, frames: Query<&FrozenFrame>) {
    let Ok(frame) = frames.get(event.entity) else {
        return;
    };
    if frame.writing.swap(true, Ordering::AcqRel) {
        return;
    }
    let path = frame.path.clone();
    let snapshot = frame.boundary.clone();
    let completed = frame.completed.clone();
    let image = event.image.clone();
    // Encoding and disk delivery can finish on a later frame. They never replace the frozen clock.
    bevy::tasks::IoTaskPool::get()
        .spawn(async move {
            let result = (|| -> anyhow::Result<Value> {
                anyhow::ensure!(
                    !path.exists(),
                    "gameplay frame output already exists: {}",
                    path.display()
                );
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                image.try_into_dynamic()?.save(&path)?;
                Ok(json!({"captured": true, "path": path, "boundary": snapshot}))
            })()
            .map_err(|error| format!("{error:#}"));
            let _ = completed.send(result);
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_out_encoder_stays_bounded_until_completion_and_releases_without_second_reply() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let (reply, received) = mpsc::channel();
        let (completed, receiver) = mpsc::channel();
        world.insert_resource(FrameCaptures {
            queued: None,
            active: Some(Active {
                request: Request {
                    id: 9,
                    path: PathBuf::from("unused.png"),
                    pcm_path: None,
                    reply: reply.clone(),
                    deadline: Instant::now(),
                    boundary: None,
                },
                entity,
                completed: Mutex::new(receiver),
                writing: Arc::new(AtomicBool::new(true)),
                timed_out: false,
            }),
        });
        capture(&mut world);
        assert!(!received.try_recv().unwrap().ok);
        assert!(
            world
                .resource_mut::<FrameCaptures>()
                .arm(10, PathBuf::from("next.png"), None, reply.clone(), None)
                .is_err()
        );
        completed
            .send(Err("encoder finished after deadline".into()))
            .unwrap();
        capture(&mut world);
        assert!(received.try_recv().is_err());
        assert!(
            world
                .resource_mut::<FrameCaptures>()
                .arm(10, PathBuf::from("next.png"), None, reply, None)
                .is_ok()
        );
    }
}
