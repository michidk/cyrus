use super::*;

/// A living receiver with a combat record, running `DragonBreath` once with the given combat flags.
fn dragon_breath_once(flags: u8) -> (anyhow::Result<i32>, usize, f64) {
    use cyrus_script::script::{ScriptInputs, ScriptStage};
    let (mut world, _, _) = movement_fixture(
        18,
        false,
        true,
        |scripts| {
            let id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name: Native::DragonBreath,
                param_count: 0,
                kind: CallKind::Function,
            });
            scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
                assign_call(1, id, Native::DragonBreath, &[]).operation,
                Operation::End { target: 0 },
            ]
            .into_iter()
            .enumerate()
            .map(|(index, operation)| {
                (
                    index as u32,
                    Step {
                        next: index as u32 + 1,
                        operation,
                    },
                )
            })
            .collect();
        },
        |saved| {
            for index in 0..2 {
                saved["actors"][index]["control"]["combat"] =
                    serde_json::to_value(cyrus_script::script::CombatControlState::default())
                        .unwrap();
                saved["actors"][index]["control"]["combat"]["flags"] = flags.into();
            }
            saved["actors"][0]["control"]["combat"]["target"] = serde_json::json!([1, 1]);
        },
    );
    let dragon = world.id(0).unwrap();
    let census = world.actors().len();
    let [a, b] = [0, 1].map(|index| {
        world
            .spatial(world.id(index).unwrap())
            .unwrap()
            .local
            .position
    });
    let distance = (0..3)
        .map(|axis| f64::from(a[axis].wrapping_sub(b[axis])).powi(2))
        .sum::<f64>()
        .sqrt();
    let result = world
        .advance(
            dragon,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            Some(&mut cyrus_game::random::EngineRandom::from_state(1)),
            100,
        )
        .and_then(|_| world.actors()[0].runtime().unwrap().variables()[0].integer());
    (result, world.actors().len() - census, distance)
}

#[test]
fn a_living_dragon_without_the_targeting_flag_keeps_breathing_and_creates_nothing() {
    let (result, created, _) = dragon_breath_once(0);
    assert_eq!((result.unwrap(), created), (1, 0));
}

#[test]
fn a_dragon_closer_than_its_minimum_range_to_its_target_keeps_breathing_without_fire() {
    let (result, created, distance) = dragon_breath_once(4);
    assert!(distance < f64::from(0x19000), "fixture distance {distance}");
    assert_eq!((result.unwrap(), created), (1, 0));
}

#[test]
fn a_dead_dragon_swaps_the_taken_item_for_the_given_one_and_returns_zero() {
    use cyrus_script::script::{ScriptActorSource, ScriptInputs, ScriptStage};
    let (_, mut world, _, _) = player_walking_fixture_with(|scripts| {
        let mut steps = Vec::new();
        for name in [Native::KillMe, Native::DragonBreath] {
            let id = u16::try_from(scripts.definition.functions.len()).unwrap();
            scripts.definition.functions.push(Function {
                name,
                param_count: 0,
                kind: CallKind::Function,
            });
            steps.push(assign_call(steps.len() as u32 + 1, id, name, &[]).operation);
        }
        steps.push(Operation::End { target: 0 });
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = steps
            .into_iter()
            .enumerate()
            .map(|(index, operation)| {
                (
                    index as u32,
                    Step {
                        next: index as u32 + 1,
                        operation,
                    },
                )
            })
            .collect();
    });
    let hud = cyrus_script::script::ScriptHud::default();
    hud.configure(
        &inventory_asset(&[(0x4b, 1), (1, 0)]),
        &cyrus_game::preferences::Preferences::default(),
    );
    let dragon = world.id(0).unwrap();
    world
        .advance(
            dragon,
            ScriptStage::OrdinaryMain,
            ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                ..Default::default()
            },
            Some(&mut |_, _| Ok(())),
            Some(&mut cyrus_game::random::EngineRandom::default()),
            100,
        )
        .unwrap();
    assert_eq!(world.actors()[0].runtime().unwrap().variables()[0], 0);
    let counts = hud.inventory(None).unwrap().counts;
    assert_eq!((counts[&0x4b], counts[&1]), (0, 1));
    assert_eq!(hud.active_item().unwrap(), 1);
    let player = world
        .actors()
        .iter()
        .position(|actor| matches!(actor.source, ScriptActorSource::Player))
        .unwrap();
    assert_eq!(world.actors()[player].attributes()[Attribute::Weapon], 1);
}

#[test]
fn golem_steam_acts_on_one_draw_in_four_and_needs_animation_data_then() {
    use cyrus_game::random::EngineRandom;
    let run = |state: u32| {
        let mut scripts = random_scripts(Native::Rnd, vec![], Object::Me);
        scripts.definition.functions.push(Function {
            name: Native::GolemSteam,
            param_count: 0,
            kind: CallKind::Function,
        });
        let id = u16::try_from(scripts.definition.functions.len() - 1).unwrap();
        scripts.actors.get_mut("LOGIC").unwrap().program.steps = [
            assign_call(1, id, Native::GolemSteam, &[]).operation,
            Operation::End { target: 0 },
        ]
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            (
                index as u32,
                Step {
                    next: index as u32 + 1,
                    operation,
                },
            )
        })
        .collect();
        scripts.content_id = scripts.content_identity().unwrap();
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        let mut random = EngineRandom::from_state(state);
        let actor = world.id(0).unwrap();
        world.advance(
            actor,
            cyrus_script::script::ScriptStage::OrdinaryMain,
            cyrus_script::script::ScriptInputs {
                frame_scale: Some(256),
                nominal_ticks: Some(1),
                ..Default::default()
            },
            None,
            Some(&mut random),
            100,
        )
    };
    let first_draw_acts = |state: u32| EngineRandom::from_state(state).rnd(4).unwrap() == 0;
    let acting = (0..64).find(|state| first_draw_acts(*state)).unwrap();
    let idle = (0..64).find(|state| !first_draw_acts(*state)).unwrap();
    run(idle).unwrap();
    let error = run(acting).unwrap_err();
    assert!(
        format!("{error:#}").contains("animation data unavailable"),
        "{error:#}"
    );
}
