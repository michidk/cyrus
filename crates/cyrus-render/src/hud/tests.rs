//! Headless HUD systems, input ordering, persistence and pixel-layout contracts.
use super::inventory::draw_inventory_text;
use super::*;
use cyrus_assets::surface::Surface;
use cyrus_script::player::input::{Action, Input, PlayerBindings};
use cyrus_soup::{AnimationGroup, Native};
use std::collections::BTreeMap;

fn bound(action: Action, key: KeyCode) -> PlayerBindings {
    [(action, Input::try_from(key).unwrap())]
        .into_iter()
        .collect()
}

fn frames(candle: &mut Candle, health: u8, n: usize) -> Vec<u16> {
    (0..n).map(|_| candle.advance(health)).collect()
}

#[test]
fn inventory_text_places_its_shadow_two_pixels_below_and_right() {
    let font = cyrus_text::Font {
        start: u16::from(b'5'),
        line_height: 1,
        glyphs: vec![cyrus_text::Glyph {
            enabled: true,
            width: 1,
            height: 1,
            offset: [0, 0],
            pixels: vec![1],
        }],
        palette: vec![[0, 0, 0], [63, 0, 0]],
    };
    let mut rgba = vec![0; 640 * 5 * 4];
    draw_inventory_text(&mut rgba, &font, "5", [1, 1], [255, 199].into());
    let pixel = |x: usize, y: usize| &rgba[(y * 640 + x) * 4..][..4];
    assert_eq!(pixel(1, 1), [252, 0, 0, 255]);
    assert_eq!(pixel(3, 3), [0, 0, 0, 199]);
    assert_eq!(pixel(2, 2), [0, 0, 0, 0]);
}

#[test]
fn leaving_the_world_keeps_the_hud_asset_for_a_paused_book_save() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<HudAsset>();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<HudAsset>>()
        .add(asset(0));
    app.insert_resource(HudHandle(handle))
        .add_systems(Update, despawn);
    app.update();
    assert!(bound_asset(app.world()).is_some());
}

#[test]
fn candle_levels_follow_the_published_health_table() {
    let levels: Vec<_> = [100, 91, 90, 82, 81, 54, 46, 36, 27, 9, 8, 1, 0]
        .map(candle_level)
        .into();
    assert_eq!(levels, [0, 0, 1, 1, 2, 5, 5, 7, 7, 9, 10, 10, 11]);
}

#[test]
fn candle_flickers_steps_between_levels_and_snuffs() {
    let mut candle = Candle::default();
    assert_eq!(frames(&mut candle, 100, 6), [1, 2, 3, 4, 0, 1]);
    assert_eq!(
        frames(&mut candle, 50, 10),
        [5, 11, 17, 23, 29, 30, 31, 32, 33, 34]
    );
    assert_eq!(frames(&mut candle, 100, 7), [29, 23, 17, 11, 5, 0, 1]);
    let mut candle = Candle::default();
    frames(&mut candle, 50, 1);
    let snuff = frames(&mut candle, 0, 17);
    assert_eq!(snuff[..15], (66..=80).collect::<Vec<_>>()[..]);
    assert_eq!(snuff[15..], [80, 80]);
    assert_eq!(frames(&mut candle, 100, 3), [65, 59, 53]);
}

#[test]
fn compass_frames_follow_the_published_table() {
    let frames: Vec<_> = [(0, 0), (1985, 0), (2047, 0), (1984, 0), (1024, 0), (65, 0)]
        .into_iter()
        .chain([(1, 0), (64, 0), (0, 2000), (1000, 2000)])
        .map(|(heading, world)| compass_frame(heading, world))
        .collect();
    assert_eq!(frames, [0, 0, 0, 1, 16, 30, 31, 31, 0, 17]);
}

#[test]
fn compass_frames_match_the_original_at_observed_headings() {
    // (world_compass, heading shown by the original's debug overlay, compass frame it drew):
    // START 2046/1865, ISLAND 1978/1885/1759/1705, BELLTOWR (512) 0/1983/1880/1738/1603.
    for (world, heading, drawn) in [
        (0, 2046, 0),
        (0, 1865, 2),
        (0, 1978, 1),
        (0, 1885, 2),
        (0, 1759, 4),
        (0, 1705, 5),
        (512, 0, 24),
        (512, 1983, 25),
        (512, 1880, 26),
        (512, 1738, 28),
        (512, 1603, 30),
    ] {
        assert_eq!(
            compass_frame(heading, world),
            drawn,
            "world_compass {world}, heading {heading}"
        );
    }
}

#[test]
fn show_timer_and_logbook_follow_their_triggers() {
    let mut state = HudState::new(&asset(0), None, HudPresentationAssets::default());
    let script_hud = cyrus_script::script::ScriptHud::default();
    state.set_health(80, &script_hud);
    assert_eq!(script_hud.health_feedback_ticks(), 25);
    script_hud.advance_health_feedback(true, 0, 0, 0);
    state.set_health(70, &script_hud);
    assert_eq!(
        script_hud.health_feedback_ticks(),
        50,
        "a pinned timer is kept"
    );
    script_hud.reset_health_feedback();
    assert_eq!(
        script_hud.health_feedback_ticks(),
        25,
        "explicit weapon reset replaces the pin"
    );
    state.logbook_entry();
    for _ in 0..31 {
        state.advance_frame();
    }
    assert_eq!(state.logbook_frame, Some(31));
    state.logbook_entry();
    state.advance_frame();
    assert_eq!(state.logbook_frame, None, "plays once, 32 frames");
}

#[test]
fn dialogue_band_uses_formatted_breaks_without_advancing_or_wrapping() {
    let hud = asset(0);
    assert_eq!(dialogue_lines(&hud, "one")[0].1[1], 418);
    let two = dialogue_lines(&hud, "one\ntwo");
    assert_eq!(
        two.iter().map(|(_, at)| at[1]).collect::<Vec<_>>(),
        [418, 444]
    );
    let three = dialogue_lines(&hud, "one\ntwo\nthree");
    assert_eq!(
        three.iter().map(|(_, at)| at[1]).collect::<Vec<_>>(),
        [392, 418, 444]
    );
    assert_eq!(three[2].0, "three");
}

#[test]
fn dialogue_record_selects_installed_player_or_npc_shadow_alpha() {
    let mut hud = asset(2);
    hud.font = cyrus_text::Font {
        start: 65,
        line_height: 26,
        palette: vec![[0, 0, 0], [63, 39, 1]],
        glyphs: vec![cyrus_text::Glyph {
            enabled: true,
            width: 1,
            height: 1,
            pixels: vec![1],
            ..default()
        }],
    };
    let (_, at) = dialogue_lines(&hud, "A").remove(0);
    let shadow = ((at[1] + 1) as usize * 640 + (at[0] + 1) as usize) * 4;
    let player = text_image(&hud, &[], Some(&(0, "A".into())), &[]);
    let npc = text_image(&hud, &[], Some(&(1, "A".into())), &[]);
    assert_eq!(player.data.as_ref().unwrap()[shadow + 3], 97);
    assert_eq!(npc.data.as_ref().unwrap()[shadow + 3], 199);
}

#[test]
fn menu_items_stack_from_the_origin_and_take_their_style_from_their_state() {
    let mut hud = asset(2);
    hud.font.line_height = 26;
    hud.text_style = "255,255".parse().unwrap();
    hud.player_text_style = "125,255".parse().unwrap();
    hud.used_text_style = "125,128".parse().unwrap();
    let mut menu = cyrus_script::script::MenuState::default();
    for (text, flag) in [("first", 0), ("second", 0), ("third", 1)] {
        menu.add(300, text, 0, flag, 0, "second");
    }
    assert_eq!(
        menu_lines(&hud, [35, 25], &menu),
        vec![
            ("first".into(), [35, 25], "125,255".parse().unwrap()),
            ("second".into(), [35, 51], "255,255".parse().unwrap()),
            ("third".into(), [35, 77], "125,128".parse().unwrap()),
        ],
        "the remembered text is highlighted; used items are dimmer"
    );
    hud.font.start = u16::from(b'a');
    hud.font.palette = vec![[0; 3], [63; 3]];
    hud.font.glyphs = vec![
        cyrus_text::Glyph {
            enabled: true,
            width: 1,
            height: 1,
            pixels: vec![1],
            ..default()
        };
        26
    ];
    let image = text_image(&hud, &[], None, &menu_lines(&hud, [35, 25], &menu));
    let pixels = image.data.as_ref().unwrap();
    let pixel = |y: usize| &pixels[(y * 640 + 35) * 4..][..4];
    assert_eq!(pixel(25), [124, 124, 124, 255], "normal topic colour scale");
    assert_eq!(
        pixel(51),
        [252, 252, 252, 255],
        "highlighted topic colour scale"
    );
    assert_eq!(
        pixel(77),
        [124, 124, 124, 128],
        "used topic colour and opacity"
    );
}

#[test]
fn text_lines_centre_health_under_the_candle_and_place_counts() {
    let digit = cyrus_text::Glyph {
        enabled: true,
        width: 9,
        ..default()
    };
    let mut hud = asset(2);
    hud.font = cyrus_text::Font {
        start: 48,
        glyphs: vec![digit; 10],
        ..default()
    };
    let item = HudItem {
        id: 2,
        bitmap: 26,
        count: 53,
        size: [40, 30],
        selected_size: None,
        name: None,
    };
    // "100" is 30 wide; the candle's centre minus 5 is 12 + 25 - 5 = 32.
    assert_eq!(
        text_lines(&hud, Some(100), Some(&item)),
        [("100".into(), [17, 106]), ("53".into(), [20, 394])]
    );
    assert!(text_lines(&hud, None, Some(&HudItem { count: 1, ..item })).is_empty());
}

fn asset(candle_mode: u8) -> HudAsset {
    HudAsset {
        labels: BTreeMap::new(),
        images: Vec::new(),
        candle_mode,
        candle: [12, 8],
        candle_size: [50, 98],
        pickup: [12, 386],
        prompt: [12, 436],
        override_ticks: 24,
        compass: [546, 396],
        compass_size: [73, 65],
        world_compass: BTreeMap::new(),
        powerup_slots: [[576, 6], [576, 96]],
        powerup_sizes: vec![[39, 61], [54, 61]],
        logbook: [540, 20],
        logbook_size: [73, 58],
        walk_mode: false,
        auto_defend: true,
        notices: default(),
        inventory: BTreeMap::new(),
        start_items: vec![],
        items: vec![],
        selected: None,
        bindings: [0; 15],
        font: default(),
        text_style: [255, 199].into(),
        player_text_style: [255, 97].into(),
        used_text_style: [128, 100].into(),
        torch_time: None,
        inventory_screen: Default::default(),
        island_map: Default::default(),
    }
}

fn prepare_script(app: &mut App, requests: &[(Native, &[i32])]) {
    use cyrus_assets::script::*;
    use cyrus_script::script::{ActivationActor, ScriptInputs, ScriptStage, ScriptWorld, Thread};
    use cyrus_soup::program::*;
    let player = Actor {
        header_index: 0,
        code_origin: 100,
        instances: 1,
        namespace: Some("CYRUS".into()),
        instance_variables: vec![vec![]],
        location_offsets: vec![],
        attributes: Default::default(),
        combat_taunts: vec![],
        native_hook: None,
        program: Program {
            version: 4,
            entry: 0,
            variables: vec![],
            strings: vec![],
            steps: BTreeMap::from([(
                0,
                Step {
                    next: 1,
                    operation: Operation::End { target: 0 },
                },
            )]),
        },
    };
    let mut lever = player.clone();
    lever.namespace = Some("LEVER".into());
    lever.code_origin = 0;
    lever.program.steps = BTreeMap::from([
        (
            0,
            Step {
                next: 1,
                operation: Operation::Call {
                    call: Call {
                        function_id: 1,
                        kind: CallKind::Function,
                        target: Object::Me,
                        function: Native::Activate,
                        arguments: vec![Value::Integer {
                            value: i32::from_le_bytes(*b"#OPN"),
                        }],
                    },
                },
            },
        ),
        (
            1,
            Step {
                next: 2,
                operation: Operation::End { target: 0 },
            },
        ),
    ]);
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
            ropes_load: None,
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
        actor_pool_capacity: 10,
        content_id: String::new(),
        shells: Vec::new(),
        world: 1,
        definition: Definition {
            functions: vec![
                Function {
                    name: Native::NullFunction,
                    param_count: 0,
                    kind: CallKind::Function,
                },
                Function {
                    name: Native::Activate,
                    param_count: 1,
                    kind: CallKind::Function,
                },
            ],
            flags: vec![],
            references: vec![],
            attributes: vec![],
        },
        actors: BTreeMap::from([("CYRUS".into(), player), ("LEVER".into(), lever)]),
        instances: vec![Instance {
            object_id: 1,
            actor: "LEVER".into(),
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
        }],
        unsupported: BTreeMap::new(),
    };
    if !requests.is_empty() {
        let mut steps = BTreeMap::new();
        for (index, (function, arguments)) in requests.iter().enumerate() {
            let function_id = scripts.definition.functions.len() as u16;
            scripts.definition.functions.push(Function {
                name: *function,
                param_count: arguments.len().try_into().unwrap(),
                kind: CallKind::Function,
            });
            steps.insert(
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation: Operation::Call {
                        call: Call {
                            function_id,
                            kind: CallKind::Function,
                            target: Object::Me,
                            function: *function,
                            arguments: arguments
                                .iter()
                                .map(|value| Value::Integer { value: *value })
                                .collect(),
                        },
                    },
                },
            );
        }
        let end = requests.len() as u32;
        steps.insert(
            end,
            Step {
                next: end + 1,
                operation: Operation::End { target: 0 },
            },
        );
        scripts.actors.get_mut("LEVER").unwrap().program.steps = steps;
    }
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let state = ActivationActor {
        state: cyrus_script::script::LivingState::ORDINARY,
        on_rope: false,
        water_state: Surface::UNKNOWN,
        torch_burning: 0,
        weapon_assigned: false,
        weapon_drawn: false,
        animation: Some(cyrus_script::script::ActivationAnimation {
            group: Some(AnimationGroup::PANIC),
        }),
    };
    let inputs = ActivationInputs {
        player: world.id(1).unwrap(),
        actors: BTreeMap::from([(0, state), (1, state)]),
        labels: BTreeMap::from([("#opn".into(), "Open".into())]),
        lockout: 0,
        dialogue_open: false,
        lineup_result: 0,
        use_held: false,
        item_system_ready: true,
        shell_execution: false,
        render_disabled: false,
    };
    world
        .advance(
            world.id(0).unwrap(),
            ScriptStage::Body(Thread::Main),
            ScriptInputs {
                activation: Some(&inputs),
                hud: app
                    .world()
                    .get_resource::<cyrus_script::script::ScriptHud>(),
                ..Default::default()
            },
            None,
            None,
            10,
        )
        .unwrap();
    let mut actors = ScriptActors::default();
    actors.current = Some(world);
    app.insert_resource(actors).insert_resource(inputs);
}

#[test]
fn script_hud_renders_powerups_and_new_logbook_frame_zero_with_lockout_rules() {
    struct Store;
    impl cyrus_game::preferences::PreferenceStore for Store {
        fn load(
            &self,
            _: cyrus_game::preferences::PreferenceKey,
        ) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
        fn save(&self, _: cyrus_game::preferences::PreferenceKey, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<HudAsset>()
        .init_resource::<cyrus_script::script::ScriptHud>()
        .insert_resource(cyrus_game::preferences::Preferences::new(Store))
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1.0 / 60.0),
        ))
        .add_systems(Update, (configure_scripts, update).chain());
    let mut data = asset(0);
    data.labels.insert("TEST".into(), "New entry".into());
    data.labels.insert("MORE".into(), "Another entry".into());
    let handle = app.world_mut().resource_mut::<Assets<HudAsset>>().add(data);
    app.insert_resource(HudHandle(handle));
    app.world_mut().spawn(HudViewport);
    app.update();
    prepare_script(
        &mut app,
        &[
            (Native::ShowBitmap, &[0, 0]),
            (Native::AddLog, &[i32::from_le_bytes(*b"TEST")]),
        ],
    );
    app.update();
    assert_eq!(
        app.world().resource::<HudState>().powerups,
        [Some((0, 0)), None]
    );
    assert_eq!(app.world().resource::<HudState>().logbook_frame, Some(0));
    let visible = |app: &mut App, expected| {
        app.world_mut()
            .query::<(&HudSprite, &Visibility)>()
            .iter(app.world())
            .find(|(sprite, _)| **sprite == expected)
            .map(|(_, visibility)| *visibility)
            .unwrap()
    };
    assert_eq!(
        visible(&mut app, HudSprite::Powerup(0)),
        Visibility::Inherited
    );
    assert_eq!(visible(&mut app, HudSprite::Logbook), Visibility::Inherited);
    prepare_script(
        &mut app,
        &[(Native::AddLog, &[i32::from_le_bytes(*b"MORE")])],
    );
    app.world_mut().resource_mut::<ActivationInputs>().lockout = 1;
    app.update();
    assert_eq!(
        app.world().resource::<HudState>().logbook_frame,
        Some(1),
        "new entry does not restart active icon"
    );
    assert_eq!(visible(&mut app, HudSprite::Powerup(0)), Visibility::Hidden);
    assert_eq!(visible(&mut app, HudSprite::Logbook), Visibility::Inherited);
    for _ in 0..31 {
        app.update();
    }
    assert_eq!(app.world().resource::<HudState>().logbook_frame, None);
    assert_eq!(visible(&mut app, HudSprite::Logbook), Visibility::Hidden);
}

fn inventory_items(counts: &[(u16, u32)]) -> BTreeMap<u16, cyrus_assets::hud::InventoryItem> {
    counts
        .iter()
        .map(|&(id, count)| {
            (
                id,
                cyrus_assets::hud::InventoryItem {
                    count,
                    total: 10,
                    player_max: 10,
                    hidden: false,
                    ..Default::default()
                },
            )
        })
        .collect()
}

fn compass_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_resource::<cyrus_script::script::ScriptHud>()
        .init_asset::<HudAsset>()
        .init_resource::<cyrus_game::preferences::Preferences>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, (configure_scripts, update).chain());
    let hud = app
        .world_mut()
        .resource_mut::<Assets<HudAsset>>()
        .add(HudAsset {
            inventory: inventory_items(&[(0, 1), (1, 1)]),
            items: vec![HudItem {
                id: 0,
                bitmap: 1,
                count: 1,
                size: [1, 1],
                selected_size: None,
                name: None,
            }],
            selected: Some(0),
            ..asset(0)
        });
    app.insert_resource(HudHandle(hud));
    app.world_mut().spawn(HudViewport);
    app.world_mut().spawn((
        Player,
        PlayerPose {
            position: [0; 3],
            heading: 1024,
        },
    ));
    app.update();
    prepare_script(&mut app, &[]);
    app.insert_resource(bound(Action::Use, KeyCode::KeyE));
    app
}

#[test]
fn inventory_retains_the_last_candle_during_closing_release_waits() {
    let mut app = compass_app();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    // One ordinary update first consumes the world's pending candle-show request.
    app.update();
    app.init_resource::<cyrus_script::inventory_screen::InventoryScreen>();
    app.init_resource::<cyrus_script::logbook::LogbookScreen>();
    app.world_mut()
        .resource_mut::<cyrus_script::logbook::LogbookScreen>()
        .inventory_open = true;
    app.world_mut()
        .resource_mut::<cyrus_script::inventory_screen::InventoryScreen>()
        .draw_admitted = true;
    // Closing may already have cleared the logical timer in PreUpdate.
    app.world()
        .resource::<cyrus_script::script::ScriptHud>()
        .set_inventory_candle(false);
    app.update();
    let candle = app.world().resource::<HudState>().candle;
    assert!(app.world().resource::<HudState>().candle_visible);
    app.world_mut()
        .resource_mut::<cyrus_script::inventory_screen::InventoryScreen>()
        .draw_admitted = false;
    for _ in 0..3 {
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!(state.candle, candle);
        assert!(state.candle_visible);
        assert_eq!(
            app.world()
                .resource::<cyrus_script::script::ScriptHud>()
                .health_feedback_ticks(),
            0
        );
    }
    app.world_mut()
        .resource_mut::<cyrus_script::logbook::LogbookScreen>()
        .inventory_open = false;
    app.update();
    assert!(!app.world().resource::<HudState>().candle_visible);
}

#[test]
fn inventory_draws_advance_only_the_shared_candle_even_while_hidden() {
    let mut app = compass_app();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    app.init_resource::<cyrus_script::inventory_screen::InventoryScreen>();
    app.init_resource::<cyrus_script::logbook::LogbookScreen>();
    app.world_mut()
        .resource_mut::<cyrus_script::logbook::LogbookScreen>()
        .inventory_open = true;
    app.world()
        .resource::<cyrus_script::script::ScriptHud>()
        .set_inventory_candle(true);
    {
        let mut state = app.world_mut().resource_mut::<HudState>();
        state.health = 100;
        state.candle = Candle::default();
        state.notices = [24, 24];
        state.logbook_frame = Some(7);
        state.elapsed = 0.004;
    }
    for (admitted, lockout, expected) in [(true, 0, 1), (false, 0, 1), (true, 1, 2), (true, 0, 3)] {
        app.world_mut()
            .resource_mut::<cyrus_script::inventory_screen::InventoryScreen>()
            .draw_admitted = admitted;
        app.world_mut().resource_mut::<HudState>().lockout = lockout;
        app.update();
        let state = app.world().resource::<HudState>();
        assert_eq!(state.candle.frame, expected);
        assert_eq!(state.candle_visible, lockout == 0);
        assert_eq!(state.notices, [24, 24]);
        assert_eq!(state.logbook_frame, Some(7));
        assert_eq!(state.elapsed, 0.004);
        let (_, image, _, visibility) = app
            .world_mut()
            .query::<(&HudSprite, &ImageNode, &Node, &Visibility)>()
            .iter(app.world())
            .find(|(sprite, ..)| **sprite == HudSprite::Candle)
            .unwrap();
        if lockout == 0 {
            assert_eq!(
                image.image,
                app.world().resource::<HudState>().frames.candle[usize::from(expected)]
            );
            assert_eq!(*visibility, Visibility::Inherited);
        } else {
            assert_eq!(*visibility, Visibility::Hidden);
        }
    }
    app.world_mut()
        .resource_mut::<cyrus_script::logbook::LogbookScreen>()
        .inventory_open = false;
    app.update();
    assert_eq!(
        app.world().resource::<HudState>().candle.frame,
        4,
        "the exit-request draw survives immediate modal teardown"
    );
    assert_eq!(app.world().resource::<HudState>().elapsed, 0.004);
    app.world_mut()
        .resource_mut::<cyrus_script::inventory_screen::InventoryScreen>()
        .draw_admitted = false;
    app.world_mut().remove_resource::<ScriptActors>();
    app.world_mut().remove_resource::<ActivationInputs>();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(1.0 / 60.0),
    ));
    app.update();
    assert_eq!(
        app.world().resource::<HudState>().candle.frame,
        0,
        "world resumes the shared flicker continuation"
    );
}

#[test]
fn candle_uses_current_actor_health_on_the_first_transition_update() {
    let mut app = compass_app();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(1.0 / 60.0),
    ));
    {
        let mut state = app.world_mut().resource_mut::<HudState>();
        state.health = 100;
        state.candle.shown = Some(0);
        state.elapsed = 0.0;
    }
    // The prepared actor has health zero; the number and snuff must change together.
    app.update();
    let state = app.world().resource::<HudState>();
    assert_eq!(state.health, 0);
    assert_eq!(state.candle.frame, 66);
    app.update();
    assert_eq!(app.world().resource::<HudState>().candle.frame, 67);
}

#[test]
fn hud_timers_follow_simulation_ticks_and_hold_in_the_console() {
    let mut app = compass_app();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::ZERO,
    ));
    let mut clock =
        cyrus_script::clock::SimulationClock::new(cyrus_assets::script::ClockSettings {
            normal_frame_rate: 12,
            min_frame_rate: 6,
            max_frame_rate: 300,
            use_smooth_fps: true,
            use_smooth_divisor: true,
        })
        .unwrap();
    assert_eq!(clock.advance(0).nominal_ticks, 1);
    app.insert_resource(clock);
    {
        let mut state = app.world_mut().resource_mut::<HudState>();
        state.health = 100;
        state.notices = [24, 24];
    }
    app.update();
    let state = app.world().resource::<HudState>();
    assert_eq!(
        app.world()
            .resource::<cyrus_script::script::ScriptHud>()
            .health_feedback_ticks(),
        24
    );
    assert_eq!(state.notices, [23, 23]);
    app.update();
    let state = app.world().resource::<HudState>();
    assert_eq!(
        app.world()
            .resource::<cyrus_script::script::ScriptHud>()
            .health_feedback_ticks(),
        24,
        "an old clock step is not replayed"
    );
    assert_eq!(state.notices, [23, 23]);
    let candle = state.candle;
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs(1),
    ));
    app.init_resource::<cyrus_console::Console>();
    app.world_mut()
        .resource_mut::<cyrus_console::Console>()
        .open = true;
    app.update();
    let state = app.world().resource::<HudState>();
    assert_eq!(state.candle, candle);
    assert_eq!(
        app.world()
            .resource::<cyrus_script::script::ScriptHud>()
            .health_feedback_ticks(),
        24
    );
    assert_eq!(state.notices, [23, 23]);
}

#[test]
fn gameplay_toggles_survive_hud_recreation_and_drive_its_projection() {
    use cyrus_script::player::input::PlayerToggles;
    let mut app = compass_app();
    assert!(!app.world().resource::<PlayerToggles>().walk_mode);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Tab);
    app.update();
    assert!(app.world().resource::<PlayerToggles>().walk_mode);
    assert!(app.world().resource::<HudState>().walk_mode);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.world_mut().remove_resource::<HudState>();
    app.update();
    assert!(app.world().resource::<PlayerToggles>().walk_mode);
    assert!(app.world().resource::<HudState>().walk_mode);
    app.world_mut().resource_mut::<PlayerToggles>().walk_mode = false;
    app.world_mut().resource_mut::<PlayerToggles>().auto_defend = true;
    app.update();
    let state = app.world().resource::<HudState>();
    assert!(!state.walk_mode);
    assert!(state.auto_defend);
}

#[test]
fn fixed_toggles_cycle_and_respect_inspection_focus() {
    let mut app = compass_app();
    let original_auto_defend = app
        .world()
        .resource::<cyrus_script::player::input::PlayerToggles>()
        .auto_defend;
    for expected in [1, 2, 0] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F8);
        app.update();
        assert_eq!(app.world().resource::<HudState>().candle_mode, expected);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F7);
    app.update();
    assert_eq!(
        app.world()
            .resource::<cyrus_script::player::input::PlayerToggles>()
            .auto_defend,
        !original_auto_defend
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.insert_resource(cyrus_script::player::input::InputSuppressed(true));
    for key in [KeyCode::F7, KeyCode::F8, KeyCode::Tab] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    app.update();
    let toggles = app
        .world()
        .resource::<cyrus_script::player::input::PlayerToggles>();
    assert_eq!(toggles.auto_defend, !original_auto_defend);
    assert!(!toggles.walk_mode);
    assert_eq!(app.world().resource::<HudState>().candle_mode, 0);
}

fn use_key(app: &mut App, held: bool) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if held {
        keys.press(KeyCode::KeyE);
    } else {
        keys.release(KeyCode::KeyE);
    }
    app.update();
}

#[test]
fn compass_uses_owned_item_once_per_hold_and_retains_session_state() {
    let mut app = compass_app();
    app.update();
    assert!(!app.world().resource::<HudState>().compass_on);
    use_key(&mut app, true);
    let state = app.world().resource::<HudState>();
    assert_eq!((state.heading, state.compass_frame), (1024, Some(16)));
    assert_eq!(
        app.world()
            .resource::<cyrus_script::script::ScriptHud>()
            .inspect()["compass_on"],
        true
    );
    app.update();
    assert!(app.world().resource::<HudState>().compass_on);
    let (node, visibility) = app
        .world_mut()
        .query::<(&HudSprite, &Node, &Visibility)>()
        .iter(app.world())
        .find(|(sprite, _, _)| **sprite == HudSprite::Compass)
        .map(|(_, node, visibility)| (node.clone(), *visibility))
        .unwrap();
    assert_eq!(visibility, Visibility::Inherited);
    assert_eq!(
        (node.left, node.top, node.width, node.height),
        (
            Val::Percent(85.3125),
            Val::Percent(82.5),
            Val::Percent(11.40625),
            Val::Percent(13.541666)
        )
    );
    prepare_script(&mut app, &[(Native::LockoutPlayer, &[1])]);
    app.update();
    assert_eq!(app.world().resource::<HudState>().compass_frame, None);
    assert!(app.world().resource::<HudState>().compass_on);
    prepare_script(&mut app, &[(Native::LockoutPlayer, &[0])]);
    app.update();
    assert_eq!(app.world().resource::<HudState>().compass_frame, Some(16));
    use_key(&mut app, false);
    app.world_mut().remove_resource::<HudState>();
    prepare_script(&mut app, &[]);
    app.update();
    app.update();
    assert!(
        app.world().resource::<HudState>().compass_on,
        "world/HUD recreation retains the flag"
    );
    app.insert_resource(bound(Action::Use, KeyCode::KeyE));
    use_key(&mut app, true);
    assert!(!app.world().resource::<HudState>().compass_on);
    assert_eq!(app.world().resource::<HudState>().compass_frame, None);
}

#[test]
fn script_acquired_compass_uses_authoritative_player_without_inspection_inputs() {
    let mut app = compass_app();
    // New session starts without compass ownership, with the installed sabre selected.
    let handle = app.world().resource::<HudHandle>().0.clone();
    {
        let mut assets = app.world_mut().resource_mut::<Assets<HudAsset>>();
        let mut hud = assets.get_mut(&handle).unwrap();
        hud.inventory.get_mut(&0).unwrap().count = 0;
        hud.inventory.get_mut(&0).unwrap().player_max = 1;
        hud.selected = Some(1);
    }
    app.insert_resource(cyrus_script::script::ScriptHud::default());
    app.update();
    app.world_mut().remove_resource::<ActivationInputs>();
    use_key(&mut app, true);
    assert!(!app.world().resource::<HudState>().compass_on);
    use_key(&mut app, false);
    prepare_script(
        &mut app,
        &[(Native::AddItem, &[0, 8]), (Native::SelectItem, &[0])],
    );
    app.world_mut().remove_resource::<ActivationInputs>();
    app.update();
    let state = app.world().resource::<HudState>();
    assert_eq!((state.selected, state.count), (Some(0), Some(1)));
    assert!(
        !state.compass_on,
        "acquisition does not activate the compass"
    );
    use_key(&mut app, true);
    assert!(app.world().resource::<HudState>().compass_on);
    assert_eq!(app.world().resource::<HudState>().compass_frame, Some(16));
    app.update();
    assert!(
        app.world().resource::<HudState>().compass_on,
        "held use does not repeat"
    );
    use_key(&mut app, false);
    // Surface changes from the actor host, without a separate activation resource.
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        let world = actors.current.as_mut().unwrap();
        let (id, _) = world.player_activation_state().unwrap();
        world.set_actor_surface(id, Surface::DEEPWATER).unwrap();
    }
    use_key(&mut app, true);
    assert!(
        app.world().resource::<HudState>().compass_on,
        "deep water rejects use"
    );
    use_key(&mut app, false);
    prepare_script(&mut app, &[(Native::LockoutPlayer, &[0])]);
    app.world_mut().remove_resource::<ActivationInputs>();
    app.world_mut().remove_resource::<HudState>();
    app.update();
    app.update();
    let state = app.world().resource::<HudState>();
    assert_eq!((state.selected, state.count), (Some(0), Some(1)));
    assert!(
        state.compass_on,
        "world and HUD replacement retain live inventory"
    );
    app.insert_resource(bound(Action::Use, KeyCode::KeyE));
    use_key(&mut app, true);
    assert!(!app.world().resource::<HudState>().compass_on);
}

#[test]
fn compass_requires_known_actor_state_ownership_selection_and_current_binding() {
    let mut app = compass_app();
    let inputs = app
        .world_mut()
        .remove_resource::<ActivationInputs>()
        .unwrap();
    let world = app
        .world_mut()
        .resource_mut::<ScriptActors>()
        .current
        .take();
    use_key(&mut app, true);
    assert!(
        !app.world().resource::<HudState>().compass_on,
        "unknown gates reject use"
    );
    app.world_mut().resource_mut::<ScriptActors>().current = world;
    app.insert_resource(inputs);
    app.update();
    assert!(
        !app.world().resource::<HudState>().compass_on,
        "rejected hold stays consumed"
    );
    use_key(&mut app, false);
    app.world()
        .resource::<cyrus_script::script::ScriptHud>()
        .select_item(1)
        .unwrap();
    use_key(&mut app, true);
    assert!(!app.world().resource::<HudState>().compass_on);
    use_key(&mut app, false);
    app.world()
        .resource::<cyrus_script::script::ScriptHud>()
        .select_item(0)
        .unwrap();
    // Re-exported initial metadata cannot replace live ownership.
    let handle = app.world().resource::<HudHandle>().0.clone();
    app.world_mut()
        .resource_mut::<Assets<HudAsset>>()
        .get_mut(&handle)
        .unwrap()
        .items[0]
        .count = 0;
    app.insert_resource(bound(Action::Use, KeyCode::KeyF));
    use_key(&mut app, true);
    assert!(
        !app.world().resource::<HudState>().compass_on,
        "old binding cannot activate"
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyF);
    app.update();
    assert!(app.world().resource::<HudState>().compass_on);
    app.world_mut().resource_mut::<HudState>().player_type = 1;
    app.world_mut().remove_resource::<ActivationInputs>();
    app.world_mut().resource_mut::<ScriptActors>().current = None;
    app.update();
    assert_eq!(app.world().resource::<HudState>().compass_frame, None);
    assert!(
        app.world().resource::<HudState>().compass_on,
        "hiding does not reset the flag"
    );
}

#[test]
fn compass_early_blocks_defer_use_but_late_actor_gates_consume_it() {
    let mut app = compass_app();
    app.init_resource::<cyrus_console::Console>();
    app.world_mut()
        .resource_mut::<cyrus_console::Console>()
        .open = true;
    use_key(&mut app, true);
    assert!(
        !app.world().resource::<HudState>().compass_on,
        "console owns input"
    );
    app.world_mut()
        .resource_mut::<cyrus_console::Console>()
        .open = false;
    app.insert_resource(cyrus_script::player::input::InputSuppressed(true));
    app.update();
    assert!(!app.world().resource::<HudState>().compass_on);
    app.world_mut()
        .resource_mut::<cyrus_script::player::input::InputSuppressed>()
        .0 = false;
    app.world_mut().resource_mut::<ActivationInputs>().lockout = 1;
    app.update();
    assert!(!app.world().resource::<HudState>().compass_on);
    app.world_mut().resource_mut::<ActivationInputs>().lockout = 0;
    app.world_mut()
        .resource_mut::<ActivationInputs>()
        .actors
        .get_mut(&1)
        .unwrap()
        .state = cyrus_script::script::LivingState::TURNING_TO_HIGH_LEDGE;
    app.update();
    assert!(!app.world().resource::<HudState>().compass_on);
    app.world_mut()
        .resource_mut::<ActivationInputs>()
        .actors
        .get_mut(&1)
        .unwrap()
        .state = cyrus_script::script::LivingState::ORDINARY;
    app.update();
    assert!(
        app.world().resource::<HudState>().compass_on,
        "early blocks leave use available"
    );
    use_key(&mut app, false);
    #[derive(Clone, Copy, Debug)]
    enum CompassGate {
        ItemSystemUnavailable,
        ShellExecution,
        DeepWater,
        ScapeDeepWater,
        DrawnWeapon,
        LightingState,
        DousingState,
        LightingAnimation,
        DousingAnimation,
        BurningTorch,
    }
    for gate in [
        CompassGate::ItemSystemUnavailable,
        CompassGate::ShellExecution,
        CompassGate::DeepWater,
        CompassGate::ScapeDeepWater,
        CompassGate::DrawnWeapon,
        CompassGate::LightingState,
        CompassGate::DousingState,
        CompassGate::LightingAnimation,
        CompassGate::DousingAnimation,
        CompassGate::BurningTorch,
    ] {
        let baseline = {
            let mut inputs = app.world_mut().resource_mut::<ActivationInputs>();
            let baseline = inputs.actors[&1];
            match gate {
                CompassGate::ItemSystemUnavailable => inputs.item_system_ready = false,
                CompassGate::ShellExecution => inputs.shell_execution = true,
                CompassGate::DeepWater => {
                    inputs.actors.get_mut(&1).unwrap().water_state = Surface::DEEPWATER
                }
                CompassGate::ScapeDeepWater => {
                    inputs.actors.get_mut(&1).unwrap().water_state = Surface::SCAPEDEEPWATER
                }
                CompassGate::DrawnWeapon => {
                    let actor = inputs.actors.get_mut(&1).unwrap();
                    actor.weapon_assigned = true;
                    actor.weapon_drawn = true;
                }
                CompassGate::LightingState => {
                    inputs.actors.get_mut(&1).unwrap().state =
                        cyrus_script::script::LivingState::LIGHTING_TORCH
                }
                CompassGate::DousingState => {
                    inputs.actors.get_mut(&1).unwrap().state =
                        cyrus_script::script::LivingState::DOUSING_TORCH
                }
                CompassGate::LightingAnimation | CompassGate::DousingAnimation => {
                    inputs.actors.get_mut(&1).unwrap().animation =
                        Some(cyrus_script::script::ActivationAnimation {
                            group: Some(if matches!(gate, CompassGate::LightingAnimation) {
                                AnimationGroup::LIGHT_TORCH
                            } else {
                                AnimationGroup::UNLIGHT_TORCH
                            }),
                        })
                }
                CompassGate::BurningTorch => inputs.actors.get_mut(&1).unwrap().torch_burning = 1,
            }
            baseline
        };
        use_key(&mut app, true);
        assert!(
            app.world().resource::<HudState>().compass_on,
            "gate {gate:?} rejected"
        );
        {
            let mut inputs = app.world_mut().resource_mut::<ActivationInputs>();
            inputs.item_system_ready = true;
            inputs.shell_execution = false;
            inputs.actors.insert(1, baseline);
        }
        app.update();
        assert!(
            app.world().resource::<HudState>().compass_on,
            "gate {gate:?} consumed hold"
        );
        use_key(&mut app, false);
    }
    use_key(&mut app, true);
    assert!(!app.world().resource::<HudState>().compass_on);
}

#[test]
fn hud_prompt_wraps_whole_glyphs_before_the_viewport_edge() {
    let mut hud = asset(2);
    hud.font = cyrus_text::Font {
        start: u16::from(b'A'),
        line_height: 8,
        glyphs: vec![cyrus_text::Glyph {
            enabled: true,
            width: 2,
            height: 1,
            pixels: vec![1, 1],
            ..default()
        }],
        palette: vec![[0; 3], [63, 0, 0]],
    };
    hud.text_style = [255, 0].into();
    let image = text_image(&hud, &[("AA".into(), [638, 10])], None, &[]);
    let pixels = image.data.as_ref().unwrap();
    let pixel = |x: usize, y: usize| &pixels[(y * 640 + x) * 4..][..4];
    assert_eq!(pixel(638, 10), [252, 0, 0, 255], "exact edge fits");
    assert_eq!(pixel(639, 10), [252, 0, 0, 255]);
    assert_eq!(pixel(638, 18), [252, 0, 0, 255], "next glyph wraps");
    assert_eq!(pixel(639, 18), [252, 0, 0, 255]);
    assert_eq!(pixel(637, 18), [0; 4], "returns to the original pen x");
}

#[test]
fn ordinary_prompt_renders_without_an_inspection_resource() {
    let mut app = compass_app();
    app.world_mut().remove_resource::<ActivationInputs>();
    let handle = app.world().resource::<HudHandle>().0.clone();
    {
        let mut assets = app.world_mut().resource_mut::<Assets<HudAsset>>();
        let mut hud = assets.get_mut(&handle).unwrap();
        hud.labels.insert("#opn".into(), "AA".into());
        hud.prompt = [638, 10];
        hud.text_style = [255, 0].into();
        hud.font = cyrus_text::Font {
            start: u16::from(b'A'),
            line_height: 8,
            glyphs: vec![cyrus_text::Glyph {
                enabled: true,
                width: 2,
                height: 1,
                pixels: vec![1, 1],
                ..default()
            }],
            palette: vec![[0; 3], [63, 0, 0]],
        };
    }
    app.insert_resource(cyrus_script::script::ScriptHud::default());
    {
        let mut actors = app.world_mut().resource_mut::<ScriptActors>();
        actors.begin_new_game();
        let world = actors.current.as_mut().unwrap();
        let animation = cyrus_assets::animation::Animations {
            version: 2,
            world: 1,
            sections: BTreeMap::new(),
            models: BTreeMap::new(),
            unsupported: BTreeMap::new(),
            metadata: serde_json::json!({
                "script_content_id":world.inspect(0, 0)["content_id"],
                "actors":[{"script_name":"CYRUS","animation_groups":[{
                    "group_index":0,"anim_type":0,"anim_id":1,"commands":[
                        {"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}]
            }),
        };
        world
            .initialize_player_animation(world.id(1).unwrap(), &animation, 0, &mut |_| Ok(()))
            .unwrap();
    }
    app.update();
    assert_eq!(
        app.world().resource::<HudState>().prompt.as_deref(),
        Some("Open")
    );
    app.world_mut()
        .resource_scope(|world, mut actors: Mut<ScriptActors>| {
            let logical = actors.current.as_mut().unwrap();
            logical
                .advance(
                    logical.id(0).unwrap(),
                    cyrus_script::script::ScriptStage::Body(cyrus_script::script::Thread::Main),
                    cyrus_script::script::ScriptInputs {
                        hud: world.get_resource::<cyrus_script::script::ScriptHud>(),
                        activation_use_held: Some(false),
                        shell_execution: Some(false),
                        nominal_ticks: Some(0),
                        ..default()
                    },
                    None,
                    None,
                    10,
                )
                .unwrap();
        });
    app.update();
    assert_eq!(
        app.world().resource::<HudState>().prompt.as_deref(),
        Some("AA")
    );
    assert!(app.world().get_resource::<ActivationInputs>().is_none());
    let image = app
        .world_mut()
        .query_filtered::<&ImageNode, With<HudText>>()
        .single(app.world())
        .unwrap()
        .image
        .clone();
    let pixels = app
        .world()
        .resource::<Assets<Image>>()
        .get(&image)
        .unwrap()
        .data
        .as_ref()
        .unwrap();
    assert_eq!(
        &pixels[(18 * 640 + 638) * 4..][..8],
        [252, 0, 0, 255, 252, 0, 0, 255],
        "the production HUD update publishes the wrapped prompt raster"
    );
}

#[test]
fn item_cycle_waits_for_prompt_draw_then_updates_equal_count_icon() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_resource::<cyrus_script::script::ScriptHud>()
        .init_asset::<HudAsset>()
        .init_resource::<cyrus_game::preferences::Preferences>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, (configure_scripts, update).chain());
    let item = |id| HudItem {
        id,
        bitmap: id,
        count: 1,
        size: [1, 1],
        selected_size: None,
        name: None,
    };
    let hud = app
        .world_mut()
        .resource_mut::<Assets<HudAsset>>()
        .add(HudAsset {
            inventory: inventory_items(&[(1, 1), (2, 1)]),
            items: vec![item(1), item(2)],
            selected: Some(1),
            ..asset(0)
        });
    app.world_mut().insert_resource(HudHandle(hud));
    app.world_mut().spawn(HudViewport);
    app.update();
    app.insert_resource(bound(Action::NextItem, KeyCode::Period));
    app.update();
    prepare_script(&mut app, &[]);
    let inputs = app
        .world_mut()
        .remove_resource::<ActivationInputs>()
        .unwrap();
    app.update();
    assert_eq!(
        app.world()
            .resource::<ScriptActors>()
            .current
            .as_ref()
            .unwrap()
            .activation()
            .prompt(),
        Some("Open")
    );
    assert_eq!(app.world().resource::<HudState>().prompt, None);
    app.insert_resource(inputs);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Period);
    app.update();
    let state = app.world().resource::<HudState>();
    assert_eq!(state.selected, Some(1));
    assert_eq!(state.prompt.as_deref(), Some("Open"));
    assert_eq!(state.text, vec![("Open".into(), [12, 436])]);
    assert!(
        app.world_mut()
            .query::<(&HudSprite, &Visibility)>()
            .iter(app.world())
            .any(|(sprite, visibility)| *sprite == HudSprite::Item
                && *visibility == Visibility::Hidden)
    );
    app.update();
    assert_eq!(app.world().resource::<HudState>().selected, Some(2));
    let expected: Handle<Image> = app
        .world()
        .resource::<AssetServer>()
        .load("cache://hud/item-02.png");
    let (image, visibility) = app
        .world_mut()
        .query::<(&HudSprite, &ImageNode, &Visibility)>()
        .iter(app.world())
        .find(|(sprite, ..)| **sprite == HudSprite::Item)
        .map(|(_, node, visibility)| (node.image.clone(), *visibility))
        .unwrap();
    assert_eq!((image, visibility), (expected, Visibility::Inherited));
    app.update();
    assert_eq!(
        app.world().resource::<HudState>().selected,
        Some(2),
        "a held item key steps once per press"
    );

    {
        let mut inputs = app.world_mut().resource_mut::<ActivationInputs>();
        let player_index = inputs.player.index;
        let player = inputs.actors.get_mut(&player_index).unwrap();
        player.weapon_assigned = true;
        player.weapon_drawn = true;
        player.animation = Some(cyrus_script::script::ActivationAnimation { group: None });
    }
    app.update();
    assert_eq!(app.world().resource::<HudState>().selected, Some(2));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Period);
    app.update();
    {
        let mut inputs = app.world_mut().resource_mut::<ActivationInputs>();
        let player_index = inputs.player.index;
        inputs.actors.get_mut(&player_index).unwrap().animation = None;
    }
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Period);
    app.update();
    assert_eq!(app.world().resource::<HudState>().selected, Some(1));
}
#[test]
fn presentation_snapshot_retains_next_candle_icon_and_fractional_frame() -> anyhow::Result<()> {
    let asset = asset(2);
    let frames = HudPresentationAssets {
        candle: vec![Handle::default(); 81],
        compass: vec![Handle::default(); 32],
        logbook: vec![Handle::default(); 32],
        powerup: vec![Handle::default(); 2],
    };
    let prepared = HudState::new(&asset, None, frames);
    let mut live = prepared.clone();
    live.set_health(47, &cyrus_script::script::ScriptHud::default());
    live.logbook_entry();
    for _ in 0..9 {
        live.advance_frame();
    }
    live.elapsed = 0.01;
    live.notices = [7, 13];
    live.text = vec![("retained prompt".into(), [17, 20])];
    live.dialogue_text = Some((3, "retained dialogue".into()));
    let saved = serde_json::to_value(live.snapshot(&asset)?)?;
    let mut restored = HudState::from_snapshot(
        &asset,
        &prepared.frames,
        serde_json::from_value(saved.clone())?,
    )?;
    assert_eq!(serde_json::to_value(restored.snapshot(&asset)?)?, saved);
    for _ in 0..26 {
        live.advance_frame();
        restored.advance_frame();
        assert_eq!(restored.candle, live.candle);
        assert_eq!(restored.logbook_frame, live.logbook_frame);
    }
    for (pointer, value) in [
        ("/version", serde_json::json!(1)),
        ("/elapsed_bits", serde_json::json!(f32::NAN.to_bits())),
        ("/state/logbook_frame", serde_json::json!(32)),
        ("/state/candle/frame", serde_json::json!(81)),
    ] {
        let mut invalid = saved.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(
            HudState::from_snapshot(&asset, &prepared.frames, serde_json::from_value(invalid)?)
                .is_err()
        );
    }
    let mut changed = asset;
    changed.logbook[0] += 1;
    assert!(
        HudState::from_snapshot(&changed, &prepared.frames, serde_json::from_value(saved)?)
            .is_err()
    );
    Ok(())
}
#[test]
fn hud_is_hidden_only_for_the_frame_that_requests_the_book() {
    let mut app = App::new();
    app.insert_resource(NextState::<AppState>::Unchanged)
        .add_systems(Update, hide_for_book_capture);
    let hud = app
        .world_mut()
        .spawn((HudContent, Visibility::Inherited))
        .id();
    let visibility = |app: &App| *app.world().get::<Visibility>(hud).unwrap();
    app.update();
    assert_eq!(visibility(&app), Visibility::Inherited);
    app.insert_resource(NextState::<AppState>::Pending(AppState::Menu));
    app.update();
    assert_eq!(visibility(&app), Visibility::Hidden);
    app.insert_resource(NextState::<AppState>::Unchanged);
    app.update();
    assert_eq!(visibility(&app), Visibility::Inherited);
}
#[test]
fn inventory_artwork_and_counts_use_the_full_viewport_coordinate_space() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<HudAsset>()
        .init_asset::<crate::environment::Environment>();
    let environment = app
        .world_mut()
        .resource_mut::<Assets<crate::environment::Environment>>()
        .add(crate::environment::Environment {
            world_palette: Vec::new(),
            effects: None,
            terrain_heights: None,
            texture_scale: [1, 1],
            sky: None,
            background: [200, 190, 250],
            clipping: [1.0, 65000.0],
            fog_table: [[0; 4]; 16],
            water: None,
            terrain_lighting: None,
            model_lighting: Default::default(),
            prelighting: Default::default(),
            water_textures: Vec::new(),
            object_lights: Vec::new(),
        });
    app.insert_resource(crate::environment::EnvironmentRequest(environment));
    app.world_mut().spawn(HudViewport);
    app.add_systems(Startup, spawn_inventory)
        .add_systems(Update, inventory_draw);
    let mut hud = asset(0);
    hud.inventory_screen = [[460, 384], [0; 2], [0; 2]];
    hud.items = vec![HudItem {
        id: 4,
        bitmap: 26,
        count: 53,
        size: [80, 70],
        selected_size: Some([84, 74]),
        name: None,
    }];
    let handle = app.world_mut().resource_mut::<Assets<HudAsset>>().add(hud);
    app.insert_resource(HudHandle(handle));
    let mut screen = cyrus_script::inventory_screen::InventoryScreen::default();
    screen.open = true;
    screen.grid.open(vec![(4, 53)], Some(4));
    screen.layout = screen.grid.layout();
    app.insert_resource(screen);
    app.update();
    let (overlay, node, children) = app
        .world_mut()
        .query_filtered::<(Entity, &Node, &Children), With<InventoryOverlay>>()
        .single(app.world())
        .unwrap();
    assert_eq!(*node, place([0, 0], [640, 480]));
    let children: Vec<_> = children.iter().collect();
    assert_eq!(
        children.len(),
        4,
        "backing, border, cursor and count/name raster"
    );
    for &child in &children[1..3] {
        assert_eq!(
            app.world().get::<artwork::ArtworkFog>(child).unwrap().0,
            [200, 190, 250],
            "inventory artwork retains the installed world fog colour",
        );
    }
    let nodes: Vec<_> = children
        .iter()
        .map(|&child| app.world().get::<Node>(child).unwrap())
        .collect();
    assert_eq!(*nodes[0], place([90, 49], [460, 384]));
    assert_eq!(*nodes[1], place([90, 49], [460, 384]));
    assert_eq!(*nodes[2], place([98, 85], [84, 74]));
    assert_eq!(*nodes[3], place([0, 0], [640, 480]));
    assert_eq!(
        app.world().get::<BackgroundColor>(overlay).unwrap().0,
        Color::NONE
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(children[0]).unwrap().0,
        Color::srgba(10.0 / 255.0, 10.0 / 255.0, 10.0 / 255.0, 180.0 / 255.0)
    );
    // From the View request until the view returns, the cursor item is not drawn.
    let mut examine = cyrus_script::item_viewer::ExamineScreen::default();
    examine.inventory_request = Some(cyrus_script::item_viewer::InventoryCell {
        item: 4,
        row: 0,
        column: 0,
    });
    app.insert_resource(examine);
    app.update();
    let children = app.world().get::<Children>(overlay).unwrap();
    assert_eq!(children.len(), 3, "backing, border and text raster");
    app.world_mut()
        .resource_mut::<cyrus_script::item_viewer::ExamineScreen>()
        .inventory_request = None;
    app.world_mut()
        .resource_mut::<cyrus_script::inventory_screen::InventoryScreen>()
        .open = false;
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(overlay),
        Some(&Visibility::Hidden)
    );
}

#[test]
fn the_examine_view_shows_its_line_text_while_open() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<HudAsset>();
    app.world_mut().spawn(HudViewport);
    app.add_systems(Startup, spawn_inventory)
        .add_systems(Update, examine_text_draw);
    let mut hud = asset(0);
    hud.labels.insert("ABCD".into(), "SOME TEXT".into());
    hud.labels.insert("NONE".into(), "\u{9e}".into());
    let handle = app.world_mut().resource_mut::<Assets<HudAsset>>().add(hud);
    app.insert_resource(HudHandle(handle));
    let mut examine = cyrus_script::item_viewer::ExamineScreen::default();
    examine.open = true;
    examine.text = Some(*b"abcd");
    app.insert_resource(examine);
    let visibility = |app: &mut App| {
        *app.world_mut()
            .query_filtered::<&Visibility, With<ExamineTextOverlay>>()
            .single(app.world())
            .unwrap()
    };
    app.update();
    assert_eq!(visibility(&mut app), Visibility::Inherited);
    app.world_mut()
        .resource_mut::<cyrus_script::item_viewer::ExamineScreen>()
        .text = Some(*b"none");
    app.update();
    assert_eq!(
        visibility(&mut app),
        Visibility::Hidden,
        "0x9E text is absent"
    );
    app.world_mut()
        .resource_mut::<cyrus_script::item_viewer::ExamineScreen>()
        .text = Some(*b"abcd");
    app.update();
    assert_eq!(visibility(&mut app), Visibility::Inherited);
    app.world_mut()
        .resource_mut::<cyrus_script::item_viewer::ExamineScreen>()
        .open = false;
    app.update();
    assert_eq!(visibility(&mut app), Visibility::Hidden);
}
#[test]
fn inventory_arrows_keep_their_sizes_positions_and_layer_order() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<HudAsset>()
        .init_asset::<crate::environment::Environment>();
    let environment = app
        .world_mut()
        .resource_mut::<Assets<crate::environment::Environment>>()
        .add(crate::environment::Environment {
            world_palette: Vec::new(),
            effects: None,
            terrain_heights: None,
            texture_scale: [1, 1],
            sky: None,
            background: [0; 3],
            clipping: [1.0, 65000.0],
            fog_table: [[0; 4]; 16],
            water: None,
            terrain_lighting: None,
            model_lighting: Default::default(),
            prelighting: Default::default(),
            water_textures: Vec::new(),
            object_lights: Vec::new(),
        });
    app.insert_resource(crate::environment::EnvironmentRequest(environment));
    app.world_mut().spawn(HudViewport);
    app.add_systems(Startup, spawn_inventory)
        .add_systems(Update, inventory_draw);
    let mut hud = asset(0);
    hud.inventory_screen = [[460, 384], [11, 12], [13, 14]];
    let handle = app.world_mut().resource_mut::<Assets<HudAsset>>().add(hud);
    app.insert_resource(HudHandle(handle));
    let mut screen = cyrus_script::inventory_screen::InventoryScreen::default();
    screen.open = true;
    screen.layout.up_arrow = true;
    screen.layout.down_arrow = true;
    app.insert_resource(screen);
    app.update();
    let children: Vec<_> = app
        .world_mut()
        .query_filtered::<&Children, With<InventoryOverlay>>()
        .single(app.world())
        .unwrap()
        .iter()
        .collect();
    let projection: Vec<_> = children
        .iter()
        .map(|&entity| {
            (
                app.world().get::<Node>(entity).unwrap().clone(),
                app.world().get::<ZIndex>(entity).unwrap().0,
            )
        })
        .collect();
    assert_eq!(
        projection,
        [
            (place([90, 49], [460, 384]), 0),
            (place([90, 49], [460, 384]), 1),
            (place([313, 54], [11, 12]), 1),
            (place([313, 419], [13, 14]), 1),
            (place([0, 0], [640, 480]), 2),
        ]
    );
    for (&entity, expected) in children[1..4].iter().zip([
        "cache://hud/invbordr-00.png",
        "cache://hud/invarrow-00.png",
        "cache://hud/invarrow-01.png",
    ]) {
        let image = app.world().get::<ImageNode>(entity).unwrap();
        assert_eq!(
            app.world()
                .resource::<AssetServer>()
                .get_path(image.image.id())
                .unwrap()
                .to_string(),
            expected
        );
    }
}
#[test]
fn quiet_hud_updates_do_not_publish_gameplay_toggle_changes() {
    #[derive(Resource, Default)]
    struct Changes(Vec<bool>);
    fn observe(
        toggles: Res<cyrus_script::player::input::PlayerToggles>,
        mut changes: ResMut<Changes>,
    ) {
        changes.0.push(toggles.is_changed());
    }
    let mut app = compass_app();
    app.init_resource::<Changes>()
        .add_systems(Update, observe.after(update));
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Changes>().0.last(), Some(&false));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Tab);
    app.update();
    assert_eq!(app.world().resource::<Changes>().0.last(), Some(&true));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.update();
    assert_eq!(app.world().resource::<Changes>().0.last(), Some(&false));
}
