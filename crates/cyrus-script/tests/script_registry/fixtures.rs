use super::*;

pub(super) struct NoServices;

impl Host for NoServices {
    fn call(
        &mut self,
        _: &Target,
        _: CallKind,
        _: Native,
        _: &[Argument],
        _: cyrus_soup::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<CallResult> {
        anyhow::bail!("no native services")
    }
    fn poll(
        &mut self,
        _: u64,
        _: &[Argument],
        _: cyrus_soup::Invocation,
        _: &mut [i32],
    ) -> anyhow::Result<bool> {
        anyhow::bail!("no tasks")
    }
    fn end(&mut self, _: Thread) -> anyhow::Result<()> {
        Ok(())
    }
}

pub(super) fn cache() -> Scripts {
    let program = Program {
        version: 4,
        entry: 0,
        variables: vec![4],
        strings: vec![],
        steps: BTreeMap::from([
            (
                0,
                Step {
                    next: 1,
                    operation: Operation::Increment {
                        destination: Place::Local { index: 0 },
                        delta: 1,
                    },
                },
            ),
            (
                1,
                Step {
                    next: 2,
                    operation: Operation::Assign {
                        destination: Place::Flag { index: 0 },
                        formula: Formula {
                            first: Value::Read {
                                place: Place::Local { index: 0 },
                            },
                            rest: vec![],
                        },
                    },
                },
            ),
            (
                2,
                Step {
                    next: 3,
                    operation: Operation::End { target: 0 },
                },
            ),
        ]),
    };
    let mut scripts = Scripts {
        version: 30,
        weapons: Default::default(),
        torches: Default::default(),
        camera: Default::default(),
        combat_attacks: vec![],
        combat_defenses: vec![],
        in_combat_threshold: None,
        lineup_distance: None,
        view_distance: None,
        texture_frames: Default::default(),
        non_engaged_spacing: None,
        combat_clank_effects: [None; 2],
        node_timer: None,
        dialogue: cyrus_assets::script::DialogueSettings {
            menu_start: [35, 25],
            max_menu_items: 300,
            max_dialog_lines: 20,
            max_text_width: 500,
            menu_traverse_delay: 4,
            print_text: true,
            use_speech: true,
            max_distance: 1600,
            rtx_filename: "ENGLISH.RTX".into(),
        },
        player_distance_override: false,
        updates: cyrus_assets::script::UpdateSettings {
            rope: None,
            ropes_load: Some(true),
            fall_death_ticks: 144,
            fall_landing: None,
            dummy_model_manager: false,
            task_system: true,
            animation_system: true,
            disable_slaves: false,
            disable_master_slaves: false,
            final_version: false,
        },
        clock: cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        },
        actor_pool_capacity: 32,
        content_id: String::new(),
        shells: Vec::new(),
        world: 7,
        definition: Definition {
            functions: vec![Function {
                name: Native::NullFunction,
                param_count: 0,
                kind: CallKind::Function,
            }],
            flags: vec![Flag {
                name: "Counter".into(),
                flag_type: "NUMBER".into(),
                initial_value: 12,
            }],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::from([(
            "LOGIC".into(),
            Actor {
                header_index: 0,
                code_origin: 0,
                instances: 2,
                namespace: Some("logic".into()),
                instance_variables: vec![vec![4], vec![9]],
                location_offsets: vec![],
                program,
                attributes: Default::default(),
                combat_taunts: vec![],
                native_hook: None,
            },
        )]),
        instances: (0..2)
            .map(|id| Instance {
                object_id: id,
                actor: "LOGIC".into(),
                object_type: Default::default(),
                active: 0,
                model_name: String::new(),
                model_mode: 0,
                light_intensity: 0,
                light_radius: 0,
                light_rgb: [0; 3],
                texture: 0,
                map_position: [0; 3],
                angles: [0; 3],
            })
            .collect(),
        unsupported: BTreeMap::new(),
    };
    scripts.content_id = scripts.content_identity().unwrap();
    scripts
}

pub(super) fn assign_call(
    next: u32,
    function_id: u16,
    function: Native,
    arguments: &[i32],
) -> Step {
    Step {
        next,
        operation: Operation::Assign {
            destination: Place::Local { index: 0 },
            formula: Formula {
                first: Value::Call {
                    call: Box::new(Call {
                        function_id,
                        function,
                        kind: CallKind::Function,
                        target: Object::Me,
                        arguments: arguments
                            .iter()
                            .map(|&value| Value::Integer { value })
                            .collect(),
                    }),
                },
                rest: vec![],
            },
        },
    }
}

/// Runs the given native calls in order on one logical actor and returns its first local.
pub(super) fn run_native_calls(calls: &[(Native, &[i32])]) -> anyhow::Result<i32> {
    run_native_world(calls)?.actors()[0]
        .runtime()
        .unwrap()
        .variables()[0]
        .integer()
}

pub(super) fn run_native_world(calls: &[(Native, &[i32])]) -> anyhow::Result<ScriptWorld> {
    run_native_world_with(calls, false)
}

pub(super) fn run_native_world_with(
    calls: &[(Native, &[i32])],
    spatial: bool,
) -> anyhow::Result<ScriptWorld> {
    let mut scripts = random_scripts(Native::Rnd, vec![], Object::Me);
    scripts.definition.functions.clear();
    scripts.definition.functions.push(Function {
        name: Native::NullFunction,
        param_count: 0,
        kind: CallKind::Function,
    });
    let mut ids = BTreeMap::new();
    for (name, arguments) in calls {
        ids.entry(*name).or_insert_with(|| {
            scripts.definition.functions.push(Function {
                name: *name,
                param_count: arguments.len() as u8,
                kind: CallKind::Function,
            });
            scripts.definition.functions.len() as u16 - 1
        });
    }
    let mut steps = BTreeMap::new();
    for (index, (name, arguments)) in calls.iter().enumerate() {
        steps.insert(
            index as u32,
            assign_call(index as u32 + 1, ids[name], *name, arguments),
        );
    }
    steps.insert(
        calls.len() as u32,
        Step {
            next: calls.len() as u32 + 1,
            operation: Operation::End { target: 0 },
        },
    );
    scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps;
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    if spatial {
        world.prepare_spatial().unwrap();
    }
    let id = world.id(0).unwrap();
    world.advance(
        id,
        cyrus_script::script::ScriptStage::OrdinaryMain,
        cyrus_script::script::ScriptInputs {
            frame_scale: Some(256),
            nominal_ticks: Some(3),
            ..Default::default()
        },
        None,
        None,
        100,
    )?;
    Ok(world)
}

pub(super) fn bind_scheduler_environment(
    app: &mut bevy::prelude::App,
) -> bevy::prelude::Handle<cyrus_assets::environment::Environment> {
    use bevy::{asset::AssetApp, prelude::*};
    use cyrus_assets::environment::Environment;
    app.init_asset::<Environment>();
    let path = app
        .world()
        .resource::<cyrus_world::lifecycle::PreparedWorld>()
        .environment_path
        .clone();
    let handle = app
        .world()
        .resource::<AssetServer>()
        .load::<Environment>(path);
    app.world_mut()
        .resource_mut::<Assets<Environment>>()
        .insert(
            handle.id(),
            Environment {
                world_palette: Vec::new(),
                effects: Some(water_model_tests::effects()),
                texture_scale: [1, 1],
                sky: None,
                background: [0; 3],
                clipping: [1.0, 1000.0],
                fog_table: [[0; 4]; 16],
                terrain_heights: None,
                water: None,
                terrain_lighting: None,
                model_lighting: Default::default(),
                prelighting: Default::default(),
                water_textures: Vec::new(),
                object_lights: vec![],
            },
        )
        .unwrap();
    handle
}
