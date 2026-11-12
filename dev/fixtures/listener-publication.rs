//! Logical camera publication versus render gliding through public camera and sample services.
use bevy::{
    app::ScheduleRunnerPlugin,
    audio::{AudioPlugin, AudioSource},
    prelude::*,
};
use cyrus_audio::{
    AudioListener,
    sound::{Effect, SampleRepetitions, SoundEffects, SoundPlugin},
};
use cyrus_script::script::{ScriptInputs, ScriptStage, ScriptWorld};

use cyrus_soup::{Attribute, Native, Thread};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
struct Fixture {
    scripts: cyrus_assets::script::Scripts,
    system: String,
    wav: Vec<u8>,
    sample_rate: u32,
    bits: u16,
    loop_flag: i8,
    source_rms: f64,
    effect: usize,
}
use cyrus_script::script::camera::{
    CameraGeometry, CameraHitSelection, CameraPose, CameraUpdateSettings, GlidePlayer,
};

struct EmptyGeometry;
impl CameraGeometry for EmptyGeometry {
    fn segment(
        &mut self,
        _: [i32; 3],
        _: [i32; 3],
        _: cyrus_script::script::ScriptActorId,
    ) -> anyhow::Result<Vec<cyrus_collision::collision_mesh::IntegerSegmentContact>> {
        Ok(vec![])
    }
    fn contacts(
        &mut self,
        _: [i32; 3],
        _: cyrus_script::script::ScriptActorId,
    ) -> anyhow::Result<Vec<[i32; 3]>> {
        Ok(vec![])
    }
    fn terrain_height(&mut self, _: [i32; 2]) -> anyhow::Result<f32> {
        anyhow::bail!("controlled scene disables terrain")
    }
}

#[derive(Debug)]
struct CameraService {
    settings: CameraUpdateSettings,
    hits: RefCell<CameraHitSelection>,
    published: RefCell<Vec<(cyrus_script::script::ScriptActorId, [i32; 3])>>,
}
impl cyrus_script::script::camera::CameraUpdateService for CameraService {
    fn update(
        &self,
        state: &mut cyrus_script::script::camera::CameraState,
        bound_position: [i32; 3],
        bound_angles: [u16; 3],
    ) -> anyhow::Result<()> {
        let update = state.prepare_pose(
            &self.hits.borrow(),
            &cyrus_script::script::camera::CameraUpdateInputs {
                bound_position,
                bound_angles,
                movement: self.settings.movement,
                body_radius: self.settings.body_radius,
                body_clearance: self.settings.body_clearance,
                terrain_enabled: self.settings.terrain_enabled,
            },
            &mut EmptyGeometry,
        )?;
        *state = update.state;
        *self.hits.borrow_mut() = update.hits;
        *self.published.borrow_mut() = update.actor_positions.to_vec();
        Ok(())
    }
    fn take_published(&self) -> Vec<(cyrus_script::script::ScriptActorId, [i32; 3])> {
        std::mem::take(&mut *self.published.borrow_mut())
    }
}

#[derive(Resource)]
struct Probe {
    world: ScriptWorld,
    fixture: Fixture,
    phase: usize,
    next: f64,
    origin: [i32; 3],
    hits: CameraHitSelection,
    render_view: fn(&cyrus_script::script::PublishedCamera) -> Option<Transform>,
}

pub fn run(render_view: fn(&cyrus_script::script::PublishedCamera) -> Option<Transform>) {
    #[cfg(not(target_arch = "wasm32"))]
    let fixture: Fixture = serde_json::from_slice(
        &std::fs::read(std::env::args_os().nth(1).expect("explicit fixture path")).unwrap(),
    )
    .unwrap();
    #[cfg(target_arch = "wasm32")]
    let fixture: Fixture = {
        #[wasm_bindgen::prelude::wasm_bindgen]
        extern "C" {
            #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=globalThis)]
            fn positionalFixture() -> String;
        }
        serde_json::from_str(&positionalFixture()).unwrap()
    };
    let mut scripts = fixture.scripts.clone();
    let camera_function = scripts.definition.functions.len() as u16;
    scripts
        .definition
        .functions
        .push(cyrus_soup::program::Function {
            name: Native::CameraController,
            param_count: 0,
            kind: cyrus_soup::program::CallKind::Function,
        });
    // Diagnostic role bodies perform no gameplay work; all emitter/settings/PCM data stays installed.
    for name in ["CYRUS", "CAMERA"] {
        let mut actor = scripts.actors.values().next().unwrap().clone();
        actor.attributes[Attribute::Slave] = 0;
        actor.attributes[Attribute::Master] = 0;
        actor.native_hook = (name == "CAMERA").then(|| cyrus_soup::program::Call {
            function_id: camera_function,
            function: Native::CameraController,
            target: cyrus_soup::program::Object::Me,
            kind: cyrus_soup::program::CallKind::Function,
            arguments: vec![],
        });
        actor.program.steps = std::collections::BTreeMap::from([(
            0,
            cyrus_soup::program::Step {
                next: 1,
                operation: cyrus_soup::program::Operation::End { target: 0 },
            },
        )]);
        scripts.actors.insert(name.into(), actor);
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 29).unwrap();
    world.prepare_spatial().unwrap();
    world.refresh_families().unwrap();
    let origin = world
        .spatial(world.id(0).unwrap())
        .unwrap()
        .effective
        .unwrap()
        .position;
    let player = world.camera().unwrap().records[0].actor;
    world
        .initialize_player_spatial(player, origin, [0; 3])
        .unwrap();
    world
        .initialize_camera_pose(CameraPose {
            position: [origin[0] - 512 * 256, origin[1], origin[2] - 512 * 256],
            angles: [0; 3],
        })
        .unwrap();
    world
        .present_camera(
            GlidePlayer {
                holding_weapon: false,
                rope_attached: false,
            },
            true,
            &mut |_| Ok(()),
        )
        .unwrap();
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_millis(10))),
        AssetPlugin::default(),
        bevy::log::LogPlugin::default(),
        AudioPlugin::default(),
        SoundPlugin,
    ))
    .insert_resource(cyrus_audio::AudioReadiness { prepare: false })
    .insert_resource(cyrus_assets::sound::SoundSettings::parse(&fixture.system));
    let audio = app
        .world_mut()
        .resource_mut::<Assets<AudioSource>>()
        .add(AudioSource {
            bytes: fixture.wav.clone().into(),
        });
    let mut effects = (0..=fixture.effect).map(|_| None).collect::<Vec<_>>();
    effects[fixture.effect] = Some(Effect {
        audio,
        bits_per_sample: fixture.bits,
        base_volume: 64,
        sample_rate: fixture.sample_rate,
        repetitions: SampleRepetitions::from_sfx_flag(fixture.loop_flag),
    });
    app.insert_resource(SoundEffects(effects))
        .insert_resource(AudioListener(GlobalTransform::IDENTITY))
        .insert_resource(Probe {
            world,
            fixture,
            phase: 0,
            next: 2.0,
            origin,
            render_view,
            hits: CameraHitSelection {
                minimum_position: [0; 3],
                maximum_position: [0; 3],
                minimum_normal: [0; 3],
                maximum_normal: [0; 3],
            },
        })
        .add_systems(Update, step.before(cyrus_audio::PlayCues))
        .add_systems(Update, publish_render.after(cyrus_audio::PlayCues));
    app.run();
}
fn step(
    time: Res<Time<Real>>,
    mut p: ResMut<Probe>,
    mut sounds: cyrus_audio::sound::ScriptSounds,
    listener: Res<AudioListener>,
    mut exit: MessageWriter<AppExit>,
) {
    if time.elapsed_secs_f64() < p.next {
        return;
    }
    if p.phase == 7 {
        exit.write(AppExit::Success);
        return;
    }
    let phase = p.phase;
    let stop = phase % 2 == 1;
    if !stop && phase != 0 {
        let origin = p.origin;
        let displacement = if phase == 2 {
            256
        } else if phase == 4 {
            -256
        } else {
            512
        };
        let yaw = if phase == 2 {
            512
        } else if phase == 4 {
            1024
        } else {
            1536
        };
        let mut placement = p.world.selected_player_placement().unwrap();
        placement.position = [origin[0] + displacement * 256, origin[1], origin[2]];
        placement.orientation = [0, yaw, 0];
        p.world
            .restore_selected_player_placement(&placement)
            .unwrap();
    }
    let id = p.world.id(0).unwrap();
    sounds.with_driver(Some(id.generation), |driver, emit| {
        if stop {
            emit(cyrus_world::sound::SoundCue::Script {
                generation: id.generation,
                index: id.index,
                sound: cyrus_world::sound::ScriptSound::StopAll,
            })
            .unwrap();
        } else {
            let service = CameraService {
                settings: CameraUpdateSettings::installed(
                    &p.fixture.scripts.camera.limits,
                    256,
                    false,
                    false,
                ),
                hits: RefCell::new(p.hits.clone()),
                published: RefCell::default(),
            };
            let camera = p.world.camera().unwrap().camera_actor;
            p.world
                .advance(
                    camera,
                    ScriptStage::NativeHook,
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        camera_update: Some(&service),
                        ambient_sound: Some(driver),
                        ..Default::default()
                    },
                    None,
                    None,
                    30,
                )
                .unwrap();
            p.hits = service.hits.into_inner();
            p.world
                .present_camera(
                    GlidePlayer {
                        holding_weapon: false,
                        rope_attached: false,
                    },
                    true,
                    &mut |_| Ok(()),
                )
                .unwrap();
            p.world
                .advance(
                    id,
                    ScriptStage::Body(Thread::Main),
                    ScriptInputs {
                        nominal_ticks: Some(1),
                        frame_scale: Some(256),
                        ambient_sound: Some(driver),
                        ..Default::default()
                    },
                    Some(&mut |actor, sound| {
                        emit(cyrus_world::sound::SoundCue::Script {
                            generation: actor.generation,
                            index: actor.index,
                            sound,
                        })
                    }),
                    None,
                    30,
                )
                .unwrap();
        }
    });
    let camera = p.world.camera().unwrap();
    if !stop {
        assert_eq!(
            sounds.listener_pose(),
            Some(cyrus_world::sound::ListenerPose {
                position: camera.current.position,
                yaw: camera.current.angles[1] as u16
            })
        );
    }
    if phase == 4 {
        assert_ne!(
            camera.current.position.map(|v| v as f32 / 256.0),
            camera.render.position,
            "fixture must expose logical/render glide separation"
        );
    }
    let rendered = (p.render_view)(&cyrus_script::script::PublishedCamera {
        pose: camera.current,
        render: camera.render,
        view_angles: camera
            .render
            .angles
            .map(|angle| cyrus_world::heading::wrap(angle as u16)),
    })
    .expect("valid rendered view");
    let stage = serde_json::json!({"positional_emitters":true,"phase":phase,"stop":stop,"position":p.origin,"listener":camera.current.position,"yaw":camera.current.angles[1],"rendered_translation":rendered.translation.to_array(),"rendered_forward":rendered.forward().to_array(),"render_position":camera.render.position,"render_angles":camera.render.angles,"previous_render_translation":listener.0.translation().to_array(),"bits":p.fixture.bits,"source_rms":p.fixture.source_rms,"master":cyrus_assets::sound::SoundSettings::parse(&p.fixture.system).master_volume,"sound_distance":cyrus_assets::sound::SoundSettings::parse(&p.fixture.system).sound_distance});
    #[cfg(not(target_arch = "wasm32"))]
    println!("{stage}");
    #[cfg(target_arch = "wasm32")]
    {
        #[wasm_bindgen::prelude::wasm_bindgen]
        extern "C" {
            #[wasm_bindgen::prelude::wasm_bindgen(js_namespace=console,js_name=log)]
            fn log(s: &str);
        }
        log(&stage.to_string());
    }
    p.phase += 1;
    p.next = time.elapsed_secs_f64() + 3.0;
}

// Match the host boundary: propagated rendered pose is available to the next update,
// while a synchronous camera hook and Sound call occur before that propagation.
fn publish_render(p: Res<Probe>, mut listener: ResMut<AudioListener>) {
    let camera = p.world.camera().unwrap();
    let transform = (p.render_view)(&cyrus_script::script::PublishedCamera {
        pose: camera.current,
        render: camera.render,
        view_angles: camera
            .render
            .angles
            .map(|angle| cyrus_world::heading::wrap(angle as u16)),
    })
    .unwrap();
    listener.0 = GlobalTransform::from(transform);
}
