use super::*;

#[test]
fn menu_natives_start_empty_and_need_installed_text_to_add_items() {
    assert_eq!(
        run_native_calls(&[(Native::MenuNew, &[]), (Native::MenuSelection, &[])]).unwrap(),
        0
    );
    let error = run_native_world(&[(Native::MenuAddItem, &[1, 0, 5])])
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains(Native::MenuAddItem.name()),
        "{error:#}"
    );
    assert!(run_native_world(&[(Native::MenuNew, &[1])]).is_err());
}

#[test]
fn wait_on_dialog_uses_initialized_shared_records_and_yields_before_completion() {
    let mut scripts = wait_cache(vec![
        wait(4, Native::WaitOnDialog, CallKind::Task, vec![]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::WaitOnDialog,
        param_count: 0,
        kind: CallKind::Task,
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(
        world.globals().dialogue().unwrap().settings(),
        &scripts.dialogue
    );
    assert!(!world.globals().dialogue().unwrap().waiting());
    for index in 0..2 {
        let id = world.id(index).unwrap();
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.actors()[index].runtime().unwrap().active_tasks(), 1);
        world.step_tasks(id, Some(256)).unwrap();
        assert_eq!(world.actors()[index].runtime().unwrap().active_tasks(), 0);
        world.step(id, Thread::Main, 20).unwrap();
        assert_eq!(world.flags(), [7]);
    }
}

#[test]
fn dialogue_lines_block_other_receivers_and_reuse_only_cleared_records() {
    let mut scripts = wait_cache(vec![
        wait(4, Native::WaitOnDialog, CallKind::Task, vec![]),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::WaitOnDialog,
        param_count: 0,
        kind: CallKind::Task,
    });
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let speaker = world.id(0).unwrap();
    let listener = world.id(1).unwrap();
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    for index in 1..scripts.dialogue.max_dialog_lines as usize {
        assert_eq!(
            dialogue.begin_line(speaker, "line".into(), Some(3), 100),
            Some(index)
        );
    }
    assert_eq!(
        dialogue.begin_line(speaker, "overflow".into(), None, 100),
        None
    );
    dialogue.clear_line(None);
    assert!(dialogue.waiting());
    world.step(listener, Thread::Main, 20).unwrap();
    world.step_tasks(listener, Some(256)).unwrap();
    assert_eq!(world.actors()[1].runtime().unwrap().active_tasks(), 1);
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    dialogue.clear_line(Some(2));
    assert!(!dialogue.line_active(Some(2)));
    assert_eq!(
        dialogue.begin_line(speaker, "replacement".into(), None, 100),
        Some(2)
    );
    for index in 1..scripts.dialogue.max_dialog_lines as usize {
        dialogue.clear_line(Some(index));
    }
    assert!(!dialogue.waiting());
    world.step_tasks(listener, Some(256)).unwrap();
    world.step(listener, Thread::Main, 20).unwrap();
    assert_eq!(world.flags(), [7]);
}

#[test]
fn dialogue_sample_cleanup_excludes_reserved_player_line_and_other_slots() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let first = world.id(0).unwrap();
    let second = world.id(1).unwrap();
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    dialogue.begin_player_line(first, "player".into(), Some(2), 100);
    assert_eq!(
        dialogue.begin_line(first, "first".into(), Some(2), 100),
        Some(1)
    );
    assert_eq!(
        dialogue.begin_line(second, "second".into(), Some(2), 100),
        Some(2)
    );
    assert_eq!(
        dialogue.begin_line(second, "other".into(), Some(3), 100),
        Some(3)
    );
    dialogue.clear_sample_lines(Some(2));
    assert!(dialogue.line_active(Some(0)));
    assert!(!dialogue.line_active(Some(1)));
    assert!(!dialogue.line_active(Some(2)));
    assert!(dialogue.line_active(Some(3)));
    dialogue.begin_player_line(second, "replacement".into(), None, 100);
    dialogue.clear_sample_lines(Some(3));
    dialogue.clear_sample_lines(None);
    assert!(dialogue.waiting());
    dialogue.clear_line(Some(0));
    assert!(!dialogue.waiting());
}

#[test]
fn dialogue_sound_reset_clears_player_and_ordinary_records() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let speaker = world.id(0).unwrap();
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    dialogue.begin_player_line(speaker, "player".into(), Some(2), 100);
    let line = dialogue.begin_line(speaker, "ordinary".into(), Some(3), 200);
    assert_eq!(line, Some(1));
    dialogue.reset_active_lines();
    assert!(!dialogue.waiting());
    assert!(!dialogue.line_active(Some(0)));
    assert!(!dialogue.line_active(line));
    let state = serde_json::to_value(&*dialogue).unwrap();
    for index in [0, 1] {
        assert_eq!(state["records"][index]["text"], "");
        assert!(state["records"][index]["sample"].is_null());
    }
    assert_eq!(state["records"][0]["remaining"], 100);
    assert_eq!(state["records"][1]["remaining"], 200);
    assert_eq!(state["records"][2]["sample"], 0);
    assert_eq!(
        dialogue.begin_line(speaker, "reused".into(), None, 300),
        Some(1)
    );
}

#[test]
fn dialogue_record_updates_preserve_order_and_speech_specific_completion() {
    use cyrus_script::script::{DialogueEffects, DialoguePlayback, ScriptActorId};
    #[derive(Default)]
    struct Effects {
        events: Vec<String>,
        playing: bool,
    }
    impl DialogueEffects for Effects {
        fn inhibit_speaker(&mut self, speaker: ScriptActorId) -> anyhow::Result<()> {
            self.events.push(format!("inhibit {}", speaker.index));
            Ok(())
        }
    }
    impl DialoguePlayback for Effects {
        fn sample_playing(&mut self, sample: u8) -> anyhow::Result<bool> {
            self.events.push(format!("status {sample}"));
            Ok(self.playing)
        }
        fn stop_sample(&mut self, sample: u8) -> anyhow::Result<()> {
            self.events.push(format!("stop {sample}"));
            self.playing = false;
            Ok(())
        }
        fn refresh_volume(&mut self, _: ScriptActorId, sample: u8) -> anyhow::Result<()> {
            self.events.push(format!("volume {sample}"));
            Ok(())
        }
        fn present_line(&mut self, index: usize, text: &str) -> anyhow::Result<()> {
            self.events.push(format!("present {index} {text}"));
            Ok(())
        }
    }
    let mut world = ScriptWorld::prepare(&cache(), 1).unwrap();
    let speaker = world.id(0).unwrap();
    let dialogue = world.globals_mut().dialogue_mut().unwrap();
    dialogue.begin_player_line(speaker, "expired".into(), None, 21);
    dialogue.begin_line(speaker, "later".into(), Some(3), 42);
    let mut effects = Effects::default();
    dialogue.advance_records(1, false, &mut effects).unwrap();
    assert_eq!(
        effects.events,
        ["inhibit 0", "inhibit 0", "present 1 later"]
    );
    assert!(!dialogue.line_active(Some(0)));
    assert!(dialogue.line_active(Some(1)));
    assert_eq!(dialogue.presented_line(), Some((1, "later")));
    effects.events.clear();
    effects.playing = true;
    dialogue.begin_player_line(speaker, "silent".into(), None, 0);
    dialogue.advance_records(1, true, &mut effects).unwrap();
    assert_eq!(
        effects.events,
        [
            "inhibit 0",
            "inhibit 0",
            "status 3",
            "volume 3",
            "present 1 later"
        ]
    );
    assert!(
        dialogue.line_active(Some(0)),
        "speech mode does not expire an absent sample by countdown"
    );
    effects.playing = false;
    effects.events.clear();
    dialogue.begin_line(speaker, "unvisited".into(), Some(4), 1);
    dialogue.advance_records(1, true, &mut effects).unwrap();
    assert_eq!(effects.events, ["inhibit 0", "inhibit 0", "status 3"]);
    assert_eq!(dialogue.presented_line(), None);
    assert!(!dialogue.line_active(Some(1)));
    assert!(dialogue.line_active(Some(2)));

    let mut fresh = ScriptWorld::prepare(&cache(), 2).unwrap();
    let speaker = fresh.id(0).unwrap();
    let dialogue = fresh.globals_mut().dialogue_mut().unwrap();
    effects.events.clear();
    effects.playing = true;
    dialogue
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert_eq!(&effects.events[..2], ["status 0", "stop 0"]);
    assert_eq!(
        effects
            .events
            .iter()
            .filter(|event| *event == "status 0")
            .count(),
        20
    );
    assert_eq!(
        effects
            .events
            .iter()
            .filter(|event| event.starts_with("stop "))
            .count(),
        1
    );
    dialogue.begin_player_line(speaker, "held".into(), None, 100);
    dialogue
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(dialogue.waiting(), "held skip does not repeat cleanup");
    dialogue
        .advance(0, true, false, Some(1), &mut effects)
        .unwrap();
    dialogue
        .advance(0, true, true, Some(2), &mut effects)
        .unwrap();
    assert!(dialogue.waiting(), "only exact lockout one admits cleanup");
    dialogue
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(
        dialogue.waiting(),
        "ineligible press still consumes the edge"
    );
    dialogue
        .advance(0, true, false, Some(1), &mut effects)
        .unwrap();
    dialogue
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(!dialogue.waiting());

    let mut actors = cyrus_script::script::ScriptActors::default();
    actors.current = Some(ScriptWorld::prepare(&cache(), 3).unwrap());
    assert!(
        format!(
            "{:#}",
            actors
                .current
                .as_mut()
                .unwrap()
                .advance_dialogue(0, true, true, &mut effects)
                .unwrap_err()
        )
        .contains("lockout unavailable for skip edge")
    );
    actors
        .current
        .as_mut()
        .unwrap()
        .advance_dialogue(0, true, false, &mut effects)
        .unwrap();
    actors.begin_new_game();
    let world = actors.current.as_mut().unwrap();
    let speaker = world.id(0).unwrap();
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .begin_line(speaker, "speaker".into(), None, 100);
    assert_eq!(world.actors()[0].attributes()[Attribute::LockPos], 0);
    world
        .advance_dialogue(1, true, false, &mut effects)
        .unwrap();
    assert_eq!(world.actors()[0].attributes()[Attribute::LockPos], 1);
    assert_eq!(world.actors()[1].attributes()[Attribute::LockPos], 0);
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .clear_line(Some(1));
    world.globals_mut().dialogue_mut().unwrap().begin_line(
        ScriptActorId {
            generation: 2,
            index: 1,
        },
        "stale".into(),
        Some(5),
        100,
    );
    // A line of a speaker from an earlier world generation inhibits nobody, keeps its voice at
    // the last volume and runs out.
    effects.events.clear();
    effects.playing = true;
    world
        .advance_dialogue(1, true, false, &mut effects)
        .unwrap();
    assert!(!effects.events.iter().any(|event| event == "volume 5"));
    assert_eq!(world.actors()[1].attributes()[Attribute::LockPos], 0);
    assert!(world.dialogue_speaker_positions().unwrap().is_empty());
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(
        !world.globals().dialogue().unwrap().waiting(),
        "skip clears records without requiring a speaker pose"
    );
}

#[test]
fn topic_menu_owner_keeps_its_state_separate_from_dialogue_and_lockout() {
    let scripts = cache();
    let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
    assert_eq!(world.globals().menu_state(), 0);
    world.globals_mut().begin_topic_menu();
    assert_eq!(world.globals().menu_state(), 1);
    world.globals_mut().end_topic_menu();
    assert_eq!(world.globals().menu_state(), 0);
    assert_eq!(world.globals().lockout(), None);
    assert!(!world.globals().dialogue().unwrap().waiting());
}

#[test]
fn player_dialogue_task_uses_reserved_record_once_and_reset_bypasses_lookup() {
    use cyrus_script::script::{
        DialogueLine, DialogueSource, ScriptActorId, ScriptInputs, ScriptStage,
    };
    use std::cell::RefCell;
    #[derive(Debug, Default)]
    struct Source(
        RefCell<Vec<([u8; 4], ScriptActorId, u32)>>,
        std::cell::Cell<u32>,
    );
    impl DialogueSource for Source {
        fn update_ambient_rtx(&self, _: u8, _: [i32; 3], _: i32, _: bool) -> anyhow::Result<bool> {
            Ok(false)
        }
        fn stop_sample(&self, _: u8) -> anyhow::Result<()> {
            Ok(())
        }
        fn player_line(
            &self,
            label: [u8; 4],
            player: ScriptActorId,
            width: u32,
        ) -> anyhow::Result<std::task::Poll<Option<DialogueLine>>> {
            self.0.borrow_mut().push((label, player, width));
            if self.1.get() != 0 {
                self.1.set(self.1.get() - 1);
                return Ok(std::task::Poll::Pending);
            }
            Ok(std::task::Poll::Ready(Some(DialogueLine {
                text: "formatted line".into(),
                sample: Some(3),
                lifetime: 128,
            })))
        }
    }
    let mut scripts = wait_cache(vec![
        wait(
            4,
            Native::RTXp,
            CallKind::Task,
            vec![Value::Integer {
                value: i32::from_le_bytes(*b"TEST"),
            }],
        ),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::RTXp,
        param_count: 1,
        kind: CallKind::Task,
    });
    let mut player = scripts.actors["LOGIC"].clone();
    player.instances = 0;
    player.instance_variables.clear();
    player.namespace = Some("cyrus".into());
    scripts.actors.insert("CYRUS".into(), player);
    scripts.content_id = scripts.content_identity().unwrap();
    let mut world = ScriptWorld::prepare(&scripts, 9).unwrap();
    let speaker = world.id(0).unwrap();
    let player = world.id(2).unwrap();
    let source = Source::default();
    let inputs = ScriptInputs {
        dialogue_source: Some(&source),
        sound_reset: Some(false),
        frame_scale: Some(256),
        ..Default::default()
    };
    world
        .advance(
            speaker,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(
        *source.0.borrow(),
        [(*b"TEST", player, scripts.dialogue.max_text_width)]
    );
    let records = serde_json::to_value(world.globals().dialogue().unwrap()).unwrap();
    assert_eq!(
        records["records"][0]["owner"],
        serde_json::to_value(player).unwrap()
    );
    assert_eq!(records["records"][0]["text"], "formatted line");
    assert!(world.globals().dialogue().unwrap().line_active(Some(0)));
    world
        .advance(speaker, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(source.0.borrow().len(), 1);
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 1);
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .clear_line(Some(0));
    world
        .advance(speaker, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().active_tasks(), 0);
    world
        .advance(
            speaker,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(world.flags(), [7]);

    let mut pending_world = ScriptWorld::prepare(&scripts, 11).unwrap();
    let speaker = pending_world.id(0).unwrap();
    let pending_source = Source::default();
    pending_source.1.set(2);
    let pending_inputs = ScriptInputs {
        dialogue_source: Some(&pending_source),
        nominal_ticks: Some(17),
        ..inputs
    };
    for stage in [ScriptStage::Body(Thread::Main), ScriptStage::Tasks] {
        pending_world
            .advance(speaker, stage, pending_inputs, None, None, 20)
            .unwrap();
        assert!(!pending_world.globals().dialogue().unwrap().waiting());
        assert_eq!(
            pending_world.actors()[0].runtime().unwrap().active_tasks(),
            1
        );
    }
    pending_world
        .advance(speaker, ScriptStage::Tasks, pending_inputs, None, None, 20)
        .unwrap();
    assert!(
        pending_world
            .globals()
            .dialogue()
            .unwrap()
            .line_active(Some(0))
    );
    assert_eq!(pending_source.0.borrow().len(), 3);
    pending_world
        .advance(speaker, ScriptStage::Tasks, pending_inputs, None, None, 20)
        .unwrap();
    assert_eq!(
        pending_source.0.borrow().len(),
        3,
        "ready dialogue must not start again"
    );

    let mut reset_world = ScriptWorld::prepare(&scripts, 10).unwrap();
    let speaker = reset_world.id(0).unwrap();
    let inputs = ScriptInputs {
        sound_reset: Some(true),
        frame_scale: Some(256),
        ..Default::default()
    };
    reset_world
        .advance(
            speaker,
            ScriptStage::Body(Thread::Main),
            inputs,
            None,
            None,
            20,
        )
        .unwrap();
    assert_eq!(reset_world.actors()[0].runtime().unwrap().active_tasks(), 1);
    reset_world
        .advance(speaker, ScriptStage::Tasks, inputs, None, None, 20)
        .unwrap();
    assert_eq!(reset_world.actors()[0].runtime().unwrap().active_tasks(), 0);
    assert!(!reset_world.globals().dialogue().unwrap().waiting());
}

#[test]
fn ordered_combat_taunts_share_rng_skip_ineligible_labels_and_fall_back_only_for_death() {
    use cyrus_game::random::EngineRandom;
    let mut scripts = cache();
    let actor = scripts.actors.get_mut("LOGIC").unwrap();
    actor.combat_taunts = [
        (TauntType::HURT_BY_OPPONENT, *b"H000"),
        (TauntType::BEGIN_COMBAT, *b"A000"),
        (TauntType::BEGIN_COMBAT, [0; 4]),
        (TauntType::BEGIN_COMBAT, *b"B000"),
        (TauntType::BEGIN_COMBAT, *b"C000"),
        (TauntType::BEGIN_COMBAT, *b"D000"),
    ]
    .into_iter()
    .map(|(trigger, label)| CombatTaunt {
        trigger,
        label,
        animation: 0,
    })
    .collect();
    actor.attributes[Attribute::NumTaunts] = 6;
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let id = world.id(0).unwrap();
    let mut random = EngineRandom::default();
    let selected = world
        .select_ordered_combat_taunt(id, TauntType::BEGIN_COMBAT, &mut random)
        .unwrap()
        .unwrap();
    assert_eq!(selected.label, *b"C000");
    assert_eq!(random.state(), 662824084);
    assert!(
        world
            .select_ordered_combat_taunt(id, TauntType::UNARMED_MALE_OPPONENT, &mut random)
            .unwrap()
            .is_none()
    );
    assert_eq!(random.state(), 662824084);
    assert_eq!(
        world
            .select_ordered_combat_taunt(id, TauntType::DEATH, &mut random)
            .unwrap()
            .unwrap()
            .label,
        *b"H000"
    );
    assert_eq!(random.state(), 3295386429);
    assert!(
        world
            .select_ordered_combat_taunt(id, TauntType::ATTACK, &mut random)
            .unwrap_err()
            .to_string()
            .contains("requires timed selection")
    );
    assert_eq!(random.state(), 3295386429);
    let replacement = ScriptWorld::prepare(&scripts, 2).unwrap();
    assert!(
        replacement
            .select_ordered_combat_taunt(id, TauntType::BEGIN_COMBAT, &mut random)
            .is_err()
    );
    assert_eq!(random.state(), 3295386429);
}

#[test]
fn combat_taunt_presentation_owns_cache_dialogue_animation_and_active_result() {
    use cyrus_script::script::{CombatTauntPlayback, DialogueLine, ScriptActorId};
    #[derive(Default, Debug)]
    struct Playback {
        events: Vec<&'static str>,
        fail: Option<&'static str>,
        speech: bool,
        width: Option<u32>,
    }
    impl Playback {
        fn stage(&mut self, stage: &'static str) -> anyhow::Result<()> {
            self.events.push(stage);
            anyhow::ensure!(self.fail != Some(stage), "injected {stage}");
            Ok(())
        }
    }
    impl CombatTauntPlayback for Playback {
        fn remembered_sample(&self, _: usize) -> anyhow::Result<Option<u8>> {
            Ok(None)
        }
        fn prepare(
            &mut self,
            label: [u8; 4],
            speech: bool,
            width: Option<u32>,
        ) -> anyhow::Result<()> {
            assert_eq!(label, *b"TEST");
            self.speech = speech;
            self.width = width;
            self.stage("prepare")
        }
        fn release(&mut self, retained: &mut Option<usize>) -> anyhow::Result<()> {
            self.stage("release")?;
            *retained = None;
            Ok(())
        }
        fn load(&mut self, _: [u8; 4]) -> anyhow::Result<usize> {
            self.stage("load")?;
            Ok(4)
        }
        fn present(
            &mut self,
            index: usize,
            _: bool,
            _: Option<u32>,
        ) -> anyhow::Result<DialogueLine> {
            assert_eq!(index, 4);
            self.stage("present")?;
            Ok(DialogueLine {
                text: "Voice".into(),
                sample: Some(3),
                lifetime: 210,
            })
        }
    }
    let fixture = |print_text, use_speech| {
        let mut scripts = cache();
        scripts.dialogue.print_text = print_text;
        scripts.dialogue.use_speech = use_speech;
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::Combat] = 1;
        scripts
            .actors
            .get_mut("LOGIC")
            .unwrap()
            .combat_taunts
            .push(CombatTaunt {
                trigger: TauntType::BEGIN_COMBAT,
                label: *b"TEST",
                animation: 159,
            });
        scripts.actors.get_mut("LOGIC").unwrap().attributes[Attribute::NumTaunts] = 1;
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
        }
        scripts.definition.functions.push(Function {
            name: Native::BeginCombat,
            param_count: 0,
            kind: CallKind::Function,
        });
        scripts.actors.get_mut("LOGIC").unwrap().native_hook = Some(Call {
            function_id: 1,
            function: Native::BeginCombat,
            target: Object::Me,
            kind: CallKind::Function,
            arguments: vec![],
        });
        scripts.content_id = scripts.content_identity().unwrap();
        let data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,
            "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
            "animation_groups":[
                {"group_index":0,"anim_id":1,"anim_type":0,"commands":[{"opcode":0,"frame_reference":10},{"opcode":2,"value":0}]},
                {"group_index":159,"anim_id":2,"anim_type":0,"commands":[{"opcode":0,"frame_reference":42},{"opcode":2,"value":0}]}
            ]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let id = world.id(0).unwrap();
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        world
            .globals_mut()
            .dialogue_mut()
            .unwrap()
            .begin_player_line(id, "Prior".into(), Some(2), 999);
        (world, id)
    };
    let taunt = CombatTaunt {
        trigger: TauntType::BEGIN_COMBAT,
        label: *b"TEST",
        animation: 159,
    };
    let empty = CombatTaunt {
        trigger: TauntType::BEGIN_COMBAT,
        label: [0; 4],
        animation: 159,
    };
    let (mut world, id) = fixture(true, true);
    let mut playback = Playback::default();
    assert_eq!(
        world
            .present_combat_taunt(id, &empty, &mut playback, 0, true, &mut |_, _| Ok(()))
            .unwrap(),
        1
    );
    assert!(playback.events.is_empty());
    assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    assert_eq!(
        world
            .present_combat_taunt(id, &taunt, &mut playback, 0, true, &mut |_, _| Ok(()))
            .unwrap(),
        1
    );
    assert_eq!(playback.events, ["prepare", "release", "load", "present"]);
    assert!(playback.speech);
    assert_eq!(playback.width, Some(500));
    assert_eq!(world.animation_frame(id).unwrap(), Some(42));
    let state = world.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (state.taunt_cache, state.taunt_dialogue, state.taunt_active),
        (Some(4), Some(1), 1)
    );
    assert!(!world.globals().dialogue().unwrap().line_active(Some(0)));
    world
        .globals_mut()
        .dialogue_mut()
        .unwrap()
        .clear_line(Some(1));
    assert_eq!(
        world
            .present_combat_taunt(id, &empty, &mut playback, 0, true, &mut |_, _| Ok(()))
            .unwrap(),
        0
    );
    assert_eq!(playback.events.len(), 4);

    for (print_text, speech) in [(false, true), (true, false), (false, false)] {
        let (mut world, id) = fixture(print_text, speech);
        let mut playback = Playback::default();
        assert_eq!(
            world
                .present_combat_taunt(id, &taunt, &mut playback, 0, false, &mut |_, _| Ok(()))
                .unwrap(),
            i32::from(print_text)
        );
        assert_eq!(playback.speech, speech);
        assert_eq!(playback.width, print_text.then_some(500));
        assert_eq!(
            world.globals().dialogue().unwrap().line_active(Some(0)),
            !print_text
        );
        assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    }
    for failure in ["prepare", "release", "load"] {
        let (mut world, id) = fixture(true, true);
        let mut playback = Playback {
            fail: Some(failure),
            ..Default::default()
        };
        assert!(
            world
                .present_combat_taunt(id, &taunt, &mut playback, 0, true, &mut |_, _| Ok(()))
                .is_err()
        );
        assert_eq!(
            world.globals().dialogue().unwrap().line_active(Some(0)),
            failure != "load"
        );
        assert_eq!(world.animation_frame(id).unwrap(), Some(10));
    }
    let (mut full, full_id) = fixture(true, true);
    for _ in 1..20 {
        assert!(
            full.globals_mut()
                .dialogue_mut()
                .unwrap()
                .begin_line(full_id, "Occupied".into(), None, 50)
                .is_some()
        );
    }
    let mut full_playback = Playback::default();
    assert_eq!(
        full.present_combat_taunt(
            full_id,
            &taunt,
            &mut full_playback,
            0,
            false,
            &mut |_, _| Ok(())
        )
        .unwrap(),
        0
    );
    let state = full.actors()[0]
        .control_state()
        .unwrap()
        .combat
        .as_ref()
        .unwrap();
    assert_eq!(
        (state.taunt_cache, state.taunt_dialogue, state.taunt_active),
        (Some(4), None, 0)
    );
    assert_eq!(
        full_playback.events,
        ["prepare", "release", "load", "present"]
    );
    let mut playback = Playback::default();
    assert!(
        world
            .present_combat_taunt(
                ScriptActorId {
                    generation: 2,
                    ..id
                },
                &taunt,
                &mut playback,
                0,
                true,
                &mut |_, _| Ok(())
            )
            .is_err()
    );
    assert!(playback.events.is_empty());
    let (mut entry, id) = fixture(true, true);
    let mut playback = Playback::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    entry
        .begin_actor_combat(
            id,
            cyrus_script::script::CombatEntryInputs {
                weapon: cyrus_script::script::WeaponInputs {
                    selected_player: id,
                    item_system_ready: true,
                    shell_execution: false,
                },
                nominal_ticks: 0,
                taunt_animation_admitted: true,
                selected_player_death_override: false,
            },
            &mut random,
            &mut playback,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(playback.events, ["prepare", "release", "load", "present"]);
    assert_eq!(random.state(), 1103527590);
    assert_eq!(entry.animation_frame(id).unwrap(), Some(42));
    let control = entry.actors()[id.index].control_state().unwrap();
    assert_eq!(
        (
            control.ai_type.byte(),
            control.living_state,
            control.weapon_drawn
        ),
        (6, LivingState::ORDINARY, true)
    );
    assert_eq!(control.combat.as_ref().unwrap().taunt_active, 1);
    #[derive(Debug)]
    struct Source<'a>(std::cell::RefCell<&'a mut Playback>);
    impl cyrus_script::script::CombatTauntSource for Source<'_> {
        fn with_playback(
            &self,
            update: &mut dyn FnMut(&mut dyn CombatTauntPlayback) -> anyhow::Result<()>,
        ) -> anyhow::Result<()> {
            update(&mut **self.0.borrow_mut())
        }
    }
    let (mut native, id) = fixture(true, true);
    let mut playback = Playback::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    let source = Source(std::cell::RefCell::new(&mut playback));
    let inputs = cyrus_script::script::ScriptInputs {
        nominal_ticks: Some(0),
        combat_entry: Some(cyrus_script::script::CombatEntryInputs {
            weapon: cyrus_script::script::WeaponInputs {
                selected_player: id,
                item_system_ready: true,
                shell_execution: false,
            },
            nominal_ticks: 0,
            taunt_animation_admitted: true,
            selected_player_death_override: false,
        }),
        combat_taunts: Some(&source),
        ..Default::default()
    };
    native
        .advance(
            id,
            cyrus_script::script::ScriptStage::NativeHook,
            inputs,
            Some(&mut |_, _| Ok(())),
            Some(&mut random),
            0,
        )
        .unwrap();
    assert_eq!(random.state(), 1103527590);
    assert_eq!(native.animation_frame(id).unwrap(), Some(42));
    native
        .advance(
            id,
            cyrus_script::script::ScriptStage::NativeHook,
            inputs,
            None,
            None,
            0,
        )
        .unwrap();
    assert_eq!(
        source.0.borrow().events,
        ["prepare", "release", "load", "present"]
    );
    let (mut overridden, id) = fixture(true, true);
    let mut playback = Playback::default();
    let mut random = cyrus_game::random::EngineRandom::from_state(1);
    overridden
        .begin_actor_combat(
            id,
            cyrus_script::script::CombatEntryInputs {
                weapon: cyrus_script::script::WeaponInputs {
                    selected_player: id,
                    item_system_ready: true,
                    shell_execution: false,
                },
                nominal_ticks: 0,
                taunt_animation_admitted: true,
                selected_player_death_override: true,
            },
            &mut random,
            &mut playback,
            &mut |_, _| Ok(()),
        )
        .unwrap();
    assert_eq!(
        overridden.actors()[id.index]
            .control_state()
            .unwrap()
            .ai_type
            .byte(),
        6
    );
    assert_eq!(overridden.animation_frame(id).unwrap(), Some(10));
    assert!(playback.events.is_empty());
    assert_eq!(
        random.state(),
        1,
        "selected death override suppresses taunt RNG only"
    );
}

#[test]
fn speaker_dialogue_task_gates_on_distance_owns_a_record_and_cuts_far_lines() {
    use cyrus_script::script::{
        DialogueLine, DialogueSource, PlayerQuery, QueryInputs, ScriptActorId, ScriptInputs,
        ScriptStage,
    };
    use std::cell::RefCell;
    #[derive(Debug, Default)]
    struct Source {
        lines: RefCell<Vec<([u8; 4], ScriptActorId)>>,
        stopped: RefCell<Vec<u8>>,
    }
    impl DialogueSource for Source {
        fn update_ambient_rtx(&self, _: u8, _: [i32; 3], _: i32, _: bool) -> anyhow::Result<bool> {
            Ok(false)
        }
        fn player_line(
            &self,
            label: [u8; 4],
            speaker: ScriptActorId,
            _: u32,
        ) -> anyhow::Result<std::task::Poll<Option<DialogueLine>>> {
            self.lines.borrow_mut().push((label, speaker));
            Ok(std::task::Poll::Ready(Some(DialogueLine {
                text: "spoken".into(),
                sample: Some(3),
                lifetime: 128,
            })))
        }
        fn stop_sample(&self, sample: u8) -> anyhow::Result<()> {
            self.stopped.borrow_mut().push(sample);
            Ok(())
        }
    }
    let mut scripts = wait_cache(vec![
        wait(
            4,
            Native::Rtx,
            CallKind::Task,
            vec![Value::Integer {
                value: i32::from_le_bytes(*b"TEST"),
            }],
        ),
        set_flag(7),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.push(Function {
        name: Native::Rtx,
        param_count: 1,
        kind: CallKind::Task,
    });
    scripts.dialogue.max_distance = 1600;
    scripts.content_id = scripts.content_identity().unwrap();
    // Whole units are the Q8 distance shifted right eight bits: the limit is inclusive.
    fn inputs(source: &Source, units: i32, reset: bool) -> ScriptInputs<'_> {
        ScriptInputs {
            dialogue_source: Some(source),
            sound_reset: Some(reset),
            frame_scale: Some(256),
            nominal_ticks: Some(0),
            player_query: Some(QueryInputs {
                player: Some(PlayerQuery {
                    position: [units * 256, 0, 0],
                    heading: 0,
                }),
                distance_override: Some(false),
                fallback_position: None,
                linked_position: None,
            }),
            ..Default::default()
        }
    }
    let start = |world: &mut ScriptWorld, inputs: ScriptInputs| {
        let speaker = world.id(0).unwrap();
        world
            .advance(
                speaker,
                ScriptStage::Body(Thread::Main),
                inputs,
                None,
                None,
                20,
            )
            .unwrap();
        speaker
    };

    let mut near = ScriptWorld::prepare(&scripts, 9).unwrap();
    near.prepare_spatial().unwrap();
    let source = Source::default();
    let speaker = start(&mut near, inputs(&source, 1600, false));
    assert_eq!(*source.lines.borrow(), [(*b"TEST", speaker)]);
    let dialogue = near.globals().dialogue().unwrap();
    assert!(dialogue.line_active(Some(1)) && !dialogue.line_active(Some(0)));
    let records = serde_json::to_value(dialogue).unwrap();
    assert_eq!(
        records["records"][1]["owner"],
        serde_json::to_value(speaker).unwrap()
    );
    // The retained record stops further lookups; the task waits until the line clears.
    near.advance(
        speaker,
        ScriptStage::Tasks,
        inputs(&source, 1600, false),
        None,
        None,
        20,
    )
    .unwrap();
    assert_eq!(source.lines.borrow().len(), 1);
    assert_eq!(near.actors()[0].runtime().unwrap().active_tasks(), 1);
    // Walking beyond the limit cuts the line, stops its sample and completes the task.
    near.advance(
        speaker,
        ScriptStage::Tasks,
        inputs(&source, 1601, false),
        None,
        None,
        20,
    )
    .unwrap();
    assert_eq!(*source.stopped.borrow(), [3]);
    assert!(!near.globals().dialogue().unwrap().line_active(Some(1)));
    assert_eq!(near.actors()[0].runtime().unwrap().active_tasks(), 0);

    let mut far = ScriptWorld::prepare(&scripts, 10).unwrap();
    far.prepare_spatial().unwrap();
    let source = Source::default();
    let speaker = start(&mut far, inputs(&source, 1601, false));
    far.advance(
        speaker,
        ScriptStage::Tasks,
        inputs(&source, 1601, false),
        None,
        None,
        20,
    )
    .unwrap();
    assert!(source.lines.borrow().is_empty());
    assert!(!far.globals().dialogue().unwrap().line_active(Some(1)));
    assert_eq!(far.actors()[0].runtime().unwrap().active_tasks(), 0);

    let mut reset = ScriptWorld::prepare(&scripts, 11).unwrap();
    reset.prepare_spatial().unwrap();
    let source = Source::default();
    let speaker = start(&mut reset, inputs(&source, 0, true));
    reset
        .advance(
            speaker,
            ScriptStage::Tasks,
            inputs(&source, 0, true),
            None,
            None,
            20,
        )
        .unwrap();
    assert!(source.lines.borrow().is_empty());
    assert_eq!(reset.actors()[0].runtime().unwrap().active_tasks(), 0);
}

#[test]
fn dialog_and_menu_natives_write_their_mode_words() {
    let state = |calls: &[(Native, &[i32])]| {
        let world = run_native_world(calls).unwrap();
        (
            world.globals().dialogue_state(),
            world.globals().menu_state(),
        )
    };
    assert_eq!(state(&[(Native::DialogBegin, &[])]), (Some(1), 0));
    // DialogBegin empties the remembered topic text; the other three leave it untouched.
    let remembered = |call: Native| {
        run_native_world(&[(call, &[])])
            .unwrap()
            .globals()
            .highlighted_choice()
            .map(str::to_owned)
    };
    assert_eq!(remembered(Native::DialogBegin), Some(String::new()));
    for call in [Native::DialogEnd, Native::MenuBegin, Native::MenuEnd] {
        assert_eq!(remembered(call), None, "{call}");
    }
    assert_eq!(
        state(&[(Native::DialogBegin, &[]), (Native::DialogEnd, &[])]),
        (Some(0), 0)
    );
    assert_eq!(state(&[(Native::MenuBegin, &[])]), (None, 1));
    assert_eq!(
        state(&[(Native::MenuBegin, &[]), (Native::MenuEnd, &[])]),
        (None, 0)
    );
    assert!(run_native_calls(&[(Native::DialogBegin, &[1])]).is_err());
}

#[test]
fn timed_taunt_request_uses_live_period_shared_rng_and_synchronous_playback() {
    use cyrus_script::script::{
        CombatTauntPlayback, CombatTauntSource, DialogueLine, ScriptInputs,
    };
    #[derive(Debug, Default)]
    struct Playback {
        events: Vec<&'static str>,
        fail: bool,
    }
    impl CombatTauntPlayback for Playback {
        fn remembered_sample(&self, _: usize) -> anyhow::Result<Option<u8>> {
            Ok(None)
        }
        fn prepare(&mut self, label: [u8; 4], _: bool, _: Option<u32>) -> anyhow::Result<()> {
            assert_eq!(label, *b"TEST");
            self.events.push("prepare");
            anyhow::ensure!(!self.fail, "injected preparation failure");
            Ok(())
        }
        fn release(&mut self, retained: &mut Option<usize>) -> anyhow::Result<()> {
            self.events.push("release");
            *retained = None;
            Ok(())
        }
        fn load(&mut self, _: [u8; 4]) -> anyhow::Result<usize> {
            self.events.push("load");
            Ok(4)
        }
        fn present(&mut self, _: usize, _: bool, _: Option<u32>) -> anyhow::Result<DialogueLine> {
            self.events.push("present");
            Ok(DialogueLine {
                text: "Voice".into(),
                sample: None,
                lifetime: 210,
            })
        }
    }
    #[derive(Debug, Default)]
    struct Source(std::cell::RefCell<Playback>);
    impl CombatTauntSource for Source {
        fn with_playback(
            &self,
            update: &mut dyn FnMut(&mut dyn CombatTauntPlayback) -> anyhow::Result<()>,
        ) -> anyhow::Result<()> {
            update(&mut *self.0.borrow_mut())
        }
    }
    for fail in [false, true] {
        let mut scripts = cache();
        let actor = scripts.actors.get_mut("LOGIC").unwrap();
        actor.attributes.extend([
            (Attribute::Combat, 1),
            (Attribute::Chatty, 124),
            (Attribute::NumTaunts, 1),
        ]);
        actor.combat_taunts = vec![CombatTaunt {
            trigger: TauntType::WANT_SWITCH_IN,
            label: *b"TEST",
            animation: 0,
        }];
        for instance in &mut scripts.instances {
            instance.object_type = PlacementType::MODEL;
        }
        scripts.instances[1].map_position = [1024, 0, 0];
        scripts.content_id = scripts.content_identity().unwrap();
        let mut data = cyrus_assets::animation::Animations {
            version: 2,
            world: scripts.world,
            metadata: serde_json::json!({"script_content_id":scripts.content_id,
                "actors":[{"script_name":"LOGIC","model_load_path":"animation_table",
                "raan_entries":[{"frame_count":1,"file_path":"TEST.3D"}],
                "animation_groups":[{"group_index":0,"anim_id":1,"anim_type":0,
                    "commands":[{"opcode":0,"frame_reference":0},{"opcode":2,"value":0}]}]}]}),
            sections: Default::default(),
            models: Default::default(),
            unsupported: Default::default(),
        };
        data.metadata["mpob_objects"] = serde_json::json!([
            {"script_name":"LOGIC"}, {"script_name":"LOGIC"}
        ]);
        data.models.insert(
            "TEST".into(),
            cyrus_assets::animation::AnimationModel {
                radius: Some(1.0),
                base_positions: vec![],
                base_normals: vec![],
                faces: vec![],
                corner_normals: vec![],
                encoding: "static_3d".into(),
                positions: vec![vec![]],
                normals: vec![vec![]],
            },
        );
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        // Construction order, rather than runtime actor index, owns the allocation index.
        world
            .initialize_map_animation(world.id(1).unwrap(), &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        let id = world.id(0).unwrap();
        world
            .initialize_map_animation(id, &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .taunt_period,
            5
        );
        let collision = cyrus_assets::collision::CollisionScene::default();
        let sources = cyrus_script::script::WorldSnapshotSources {
            identity_memo: None,
            effects: None,
            player: None,
            scripts: &scripts,
            animations: &data,
            collision: &collision,
        };
        let allocation = |world: &ScriptWorld, index: usize| {
            world.actors()[index]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .allocation_index
        };
        assert_eq!((allocation(&world, 0), allocation(&world, 1)), (1, 0));
        let mut saved = serde_json::to_value(world.snapshot(&sources).unwrap()).unwrap();
        assert_eq!(saved["next_combat_index"], 2);
        let mut missing_counter = saved.clone();
        missing_counter
            .as_object_mut()
            .unwrap()
            .remove("next_combat_index");
        assert!(
            serde_json::from_value::<cyrus_script::script::WorldSnapshot>(missing_counter).is_err()
        );
        let mut continued = saved.clone();
        continued["actors"][1]["control"]["combat"] = serde_json::Value::Null;
        continued["actors"][1]["animation"] = serde_json::Value::Null;
        continued["next_combat_index"] = i32::MAX.into();
        let mut continued = ScriptWorld::from_snapshot(
            serde_json::from_value(continued).unwrap(),
            &sources,
            cyrus_script::script::WorldRestoreContext {
                generation: 2,
                globals: Default::default(),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        continued
            .initialize_map_animation(continued.id(1).unwrap(), &data, 0, &mut |_, _| Ok(()))
            .unwrap();
        assert_eq!(allocation(&continued, 1), i32::MAX);
        assert_eq!(
            serde_json::to_value(continued.snapshot(&sources).unwrap()).unwrap()["next_combat_index"],
            i32::MIN
        );

        saved["actors"][0]["control"]["combat"]["allocation_index"] = (-1).into();
        saved["actors"][0]["control"]["combat"]["action"] = 0.into();
        let mut missing = saved.clone();
        missing["actors"][0]["control"]["combat"]
            .as_object_mut()
            .unwrap()
            .remove("taunt_period");
        assert!(serde_json::from_value::<cyrus_script::script::WorldSnapshot>(missing).is_err());
        for dead in [false, true] {
            let mut blocked = saved.clone();
            if dead {
                blocked["actors"][0]["control"]["death_selector"] = 8.into();
            } else {
                blocked["actors"][0]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
            }
            let mut blocked = ScriptWorld::from_snapshot(
                serde_json::from_value(blocked).unwrap(),
                &sources,
                cyrus_script::script::WorldRestoreContext {
                    generation: 2,
                    globals: Default::default(),
                    samples: &BTreeMap::new(),
                },
            )
            .unwrap();
            let mut random = cyrus_game::random::EngineRandom::default();
            blocked
                .request_timed_combat_taunt(
                    blocked.id(0).unwrap(),
                    TauntType::WANT_SWITCH_IN,
                    1,
                    ScriptInputs::default(),
                    &mut random,
                    &mut |_, _| panic!("blocked sound"),
                )
                .unwrap();
            assert_eq!(
                random.state(),
                1,
                "dead or exact-distance boundary must not draw"
            );
        }
        // Attribute writes after construction must not replace the retained divisor.
        saved["actors"][0]["attributes"][87] = 129.into();
        world = ScriptWorld::from_snapshot(
            serde_json::from_value(saved).unwrap(),
            &sources,
            cyrus_script::script::WorldRestoreContext {
                generation: 2,
                globals: std::mem::take(world.globals_mut()),
                samples: &BTreeMap::new(),
            },
        )
        .unwrap();
        let id = world.id(0).unwrap();
        assert_eq!(world.actors()[0].attributes()[Attribute::Chatty], 129);
        assert_eq!(
            world.actors()[0]
                .control_state()
                .unwrap()
                .combat
                .as_ref()
                .unwrap()
                .taunt_period,
            5
        );
        let mut endpoint = cyrus_game::random::EngineRandom::from_state(24884);
        let error = world
            .request_timed_combat_taunt(
                id,
                TauntType::WANT_SWITCH_IN,
                1,
                ScriptInputs::default(),
                &mut endpoint,
                &mut |_, _| panic!("unsupported endpoint sound"),
            )
            .unwrap_err();
        assert!(format!("{error:#}").contains("world 7 instance 0 timed combat taunt type 9"));
        assert!(format!("{error:#}").contains("one-past access unsupported"));
        assert_eq!(endpoint.state(), 2147445597);
        let source = Source::default();
        source.0.borrow_mut().fail = fail;
        let inputs = ScriptInputs {
            nominal_ticks: Some(0),
            combat_taunts: Some(&source),
            ..Default::default()
        };
        let mut random = cyrus_game::random::EngineRandom::default();
        let initial = random.state();
        // Allocation index -1 is off-period; action and attack state zero must not admit it.
        world
            .request_timed_combat_taunt(
                id,
                TauntType::WANT_SWITCH_IN,
                0,
                inputs,
                &mut random,
                &mut |_, _| Ok(()),
            )
            .unwrap();
        assert!(source.0.borrow().events.is_empty());
        let mut expected = cyrus_game::random::EngineRandom::from_state(initial);
        for _ in 0..15 {
            expected.draw();
        }
        assert_eq!(random.state(), expected.state());
        let result = world.request_timed_combat_taunt(
            id,
            TauntType::WANT_SWITCH_IN,
            1,
            inputs,
            &mut random,
            &mut |_, _| Ok(()),
        );
        for _ in 0..15 {
            expected.draw();
        }
        assert_eq!(random.state(), expected.state());
        let combat = world.actors()[0]
            .control_state()
            .unwrap()
            .combat
            .as_ref()
            .unwrap();
        if fail {
            assert!(format!("{:#}", result.unwrap_err()).contains("injected preparation failure"));
            assert_eq!(combat.flags.bits() & 0x40, 0);
            assert_eq!(source.0.borrow().events, ["prepare"]);
        } else {
            result.unwrap();
            assert_eq!(combat.flags.bits() & 0x40, 0x40);
            assert_eq!(
                source.0.borrow().events,
                ["prepare", "release", "load", "present"]
            );
            assert_eq!(combat.taunt_cache, Some(4));
            // Type-nine retained flag rejects without RNG or playback services.
            world
                .request_timed_combat_taunt(
                    id,
                    TauntType::WANT_SWITCH_IN,
                    1,
                    ScriptInputs::default(),
                    &mut random,
                    &mut |_, _| panic!("rejected sound"),
                )
                .unwrap();
            assert_eq!(random.state(), expected.state());
        }
    }
}

#[test]
fn dialogue_preparation_reads_literal_labels_without_running_dormant_scripts() {
    let mut scripts = wait_cache(vec![
        wait(
            4,
            Native::Rtx,
            CallKind::Task,
            vec![Value::Integer {
                value: i32::from_le_bytes(*b"BETA"),
            }],
        ),
        wait(
            5,
            Native::RTXpAnim,
            CallKind::Task,
            vec![
                Value::Integer {
                    value: i32::from_le_bytes(*b"ALFA"),
                },
                Value::Integer { value: 0 },
                Value::Integer { value: 0 },
                Value::Integer { value: 0 },
            ],
        ),
        wait(
            4,
            Native::Rtx,
            CallKind::Task,
            vec![Value::Integer {
                value: i32::from_le_bytes(*b"BETA"),
            }],
        ),
        wait(
            4,
            Native::Rtx,
            CallKind::Task,
            vec![Value::Read {
                place: Place::Local { index: 0 },
            }],
        ),
        wait(
            1,
            Native::Wait,
            CallKind::Task,
            vec![Value::Integer {
                value: i32::from_le_bytes(*b"NOPE"),
            }],
        ),
        Operation::End { target: 0 },
    ]);
    scripts.definition.functions.extend([
        Function {
            name: Native::Rtx,
            param_count: 1,
            kind: CallKind::Task,
        },
        Function {
            name: Native::RTXpAnim,
            param_count: 4,
            kind: CallKind::Task,
        },
    ]);
    scripts.instances.clear();
    scripts.content_id = scripts.content_identity().unwrap();
    let world = ScriptWorld::prepare(&scripts, 1).unwrap();
    let before = world.inspect(0, 64);
    assert_eq!(world.dialogue_labels(), vec![*b"ALFA", *b"BETA"]);
    assert_eq!(world.dialogue_labels(), vec![*b"ALFA", *b"BETA"]);
    assert_eq!(world.inspect(0, 64), before);
}

#[test]
fn physical_topic_navigation_survives_a_mod_locomotion_claim() -> anyhow::Result<()> {
    use bevy::prelude::*;
    use cyrus_game::{AppState, fidelity::Fidelity};
    use cyrus_script::player::{PlayerPlugin, input::*};
    use cyrus_script::script::{MenuInput, MenuState, ScriptActors};
    for mod_direction in [false, true] {
        let mut world = ScriptWorld::prepare(&cache(), 1)?;
        world.globals_mut().begin_topic_menu();
        let mut actors = ScriptActors::default();
        actors.current = Some(world);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<AppState>()
            .insert_resource(State::new(AppState::InGame))
            .init_resource::<Fidelity>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(PlayerBindings::from_codes([72, 80, 75, 77, 42, 57, 56, 29]).unwrap())
            .insert_resource(actors)
            .add_plugins(PlayerPlugin);
        let mut menu = MenuState::default();
        menu.add(10, "first", 0, 0, 10, "");
        menu.add(10, "second", 1, 0, 20, "");
        let claim = LocomotionClaim {
            heading: 0,
            travel: 0,
            speed: 0.0,
            gait: Gait::Walk,
            jump: false,
            tuning: default(),
        };
        let mut poll = |app: &mut App, tick| {
            app.insert_resource(LocomotionClaimed(Some(claim)));
            app.update();
            let held = app.world().resource::<PlayerInput>();
            menu.step(MenuInput {
                up: held.forward,
                down: held.backward,
                select: held.action,
                tick,
                elapsed_delay: true,
                traverse_delay: 4,
            })
        };
        if mod_direction {
            app.insert_resource(MovementWish(Some(Wish {
                direction: Vec2::NEG_Y,
                jump: false,
                wasd: true,
            })));
        } else {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ArrowDown);
        }
        app.insert_resource(CameraOrbit {
            enabled: true,
            turn: 100.0,
        });
        assert!(poll(&mut app, 1));
        let look = app.world().resource::<ModernLook>();
        assert_eq!(
            look.turn, 0.0,
            "dialogue must not accumulate camera rotation"
        );
        assert!(!look.orbit);
        app.insert_resource(MovementWish(None));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ArrowDown);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        assert!(poll(&mut app, 2));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ControlLeft);
        assert!(!poll(&mut app, 3));
        assert_eq!(
            menu.selection(),
            20,
            "physical Down must select the non-first topic"
        );
        // The script rebuilds used topics with the alternate-font flag and remembers the highlight.
        menu.clear();
        menu.add(10, "first", 0, 0, 10, "second");
        menu.add(10, "second", 1, 1, 20, "second");
        assert_eq!(menu.highlighted(), 1);
        assert_eq!(menu.items()[1].flag, 1);
    }
    Ok(())
}
