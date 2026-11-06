//! Installed item use scripts run through the production shell update, with the player animation
//! advancing, and leave the health, flags and inventory counts their authored steps write.
use cyrus_assets::{animation::Animations, hud::HudAsset, script::Scripts};
use cyrus_game::script::{
    AnimationData, DialogueLine, DialogueSource, ScriptActorId, ScriptActorSource, ScriptHud,
    ScriptInputs, ScriptStage, ScriptWorld,
};
use cyrus_script::Attribute;

#[derive(Debug)]
struct NoLines;
impl DialogueSource for NoLines {
    fn player_line(
        &self,
        _: [u8; 4],
        _: ScriptActorId,
        _: u32,
    ) -> anyhow::Result<std::task::Poll<Option<DialogueLine>>> {
        Ok(std::task::Poll::Ready(None))
    }
    fn stop_sample(&self, _: u8) -> anyhow::Result<()> {
        Ok(())
    }
    fn update_ambient_rtx(&self, _: u8, _: [i32; 3], _: i32, _: bool) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// Script, final health (from 40), flags set beyond the START baseline, changed counts
/// `(item, before, after)`. Every potion script leaves health alone at potion state zero and swaps
/// the full bottle for the empty one; the bottle drop of an unowned item changes nothing.
#[allow(clippy::type_complexity)]
const EXPECTED: [(&str, u8, &[usize], &[(u32, i64, i64)]); 13] = [
    ("ihealth", 55, &[364], &[(4, 6, 5)]),
    ("iironsk", 40, &[367], &[]),
    ("istrenth", 40, &[366], &[]),
    ("iblood", 40, &[], &[(19, 0, 1)]),
    ("imilk", 40, &[], &[(21, 0, 1)]),
    ("iecto", 40, &[], &[(23, 0, 1)]),
    ("isap", 40, &[], &[(25, 0, 1)]),
    ("ishovel", 40, &[], &[]),
    // The flask script loops, throwing while its flag is set; it ends when used again.
    ("iflask", 40, &[361], &[]),
    ("iflag", 40, &[], &[]),
    ("ibotwat", 45, &[364], &[(64, 0, 1)]),
    ("ibotaloe", 55, &[364], &[(64, 0, 1)]),
    ("ibandage", 70, &[364], &[(69, 0, 1)]),
];
/// The item a script selects after its swap; scripts without an entry leave the selection alone.
const SELECTED: [(&str, u16); 7] = [
    ("iblood", 19),
    ("imilk", 21),
    ("iecto", 23),
    ("isap", 25),
    ("ibotwat", 64),
    ("ibotaloe", 64),
    ("ibandage", 69),
];
const BASELINE_FLAGS: [usize; 6] = [0, 194, 195, 231, 352, 353];

#[test]
fn installed_item_scripts_run_to_completion() {
    let root = std::env::var_os("REDGUARD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
    let Ok(install) = cyrus_assets::install::GameInstall::locate(Some(&root)) else {
        eprintln!("skipped: no install (item use scripts)");
        return;
    };
    let files = cyrus_assets::native::Files::open(&install).unwrap();
    let prepared = cyrus_assets::game::prepare(&files, "START.scripts").unwrap();
    let hud_files = cyrus_assets::game::prepare(&files, "hud/overlay.hud").unwrap();
    let scripts: Scripts = serde_json::from_slice(&prepared["START.scripts"]).unwrap();
    let animations: Animations = serde_json::from_slice(&prepared["START.animations"]).unwrap();
    let asset: HudAsset = serde_json::from_slice(&hud_files["hud/overlay.hud"]).unwrap();
    for (name, health, flags, counts) in EXPECTED {
        let mut world = ScriptWorld::prepare(&scripts, 1).unwrap();
        world.prepare_spatial().unwrap();
        let player = world
            .id(world
                .actors()
                .iter()
                .position(|actor| matches!(actor.source, ScriptActorSource::Player))
                .unwrap())
            .unwrap();
        world
            .initialize_player_spatial(player, [0; 3], [0; 3])
            .unwrap();
        world
            .initialize_player_animation(player, &animations, 0, &mut |_| Ok(()))
            .unwrap();
        world.set_attribute_for_test(player, Attribute::Health, 40);
        world.globals_mut().initialize_selected_health_for_test(40);
        let hud = ScriptHud::default();
        hud.configure(&asset, &cyrus_game::preferences::Preferences::default());
        let before = hud.inventory(None).unwrap().counts;
        let selected_before = hud.inventory(None).unwrap().selected;
        world.spawn_shell(name).unwrap();
        let mut random = cyrus_game::random::EngineRandom::default();
        let mut finished = false;
        for tick in 0..600 {
            let inputs = ScriptInputs {
                hud: Some(&hud),
                nominal_ticks: Some(1),
                frame_scale: Some(256),
                sound_reset: Some(false),
                dialogue_source: Some(&NoLines),
                animations: Some(AnimationData(&animations)),
                ..Default::default()
            };
            let result = world
                .advance(
                    player,
                    ScriptStage::Animation,
                    inputs,
                    Some(&mut |_, _| Ok(())),
                    Some(&mut random),
                    30,
                )
                .and_then(|()| world.update_shells(inputs, &mut random, &mut |_, _| Ok(()), 30));
            result.unwrap_or_else(|error| panic!("{name} tick {tick}: {error:#}"));
            if let Some(failure) = world.actors().last().and_then(|actor| actor.failure()) {
                panic!("{name} tick {tick}: {failure}");
            }
            if world.running_shells(name) == 0 {
                finished = true;
                break;
            }
        }
        let after = hud.inventory(None).unwrap().counts;
        let changed: Vec<_> = after
            .iter()
            .map(|(id, count)| {
                (
                    u32::from(*id),
                    i64::from(before.get(id).copied().unwrap_or(0)),
                    i64::from(*count),
                )
            })
            .filter(|(_, was, now)| was != now)
            .collect();
        assert_eq!(finished, name != "iflask", "{name}: completion");
        assert_eq!(
            world.actors()[player.index].attributes()[Attribute::Health],
            health,
            "{name}: health"
        );
        let set: Vec<usize> = world
            .flags()
            .iter()
            .enumerate()
            .filter(|(_, value)| **value != 0)
            .map(|(index, _)| index)
            .filter(|index| !BASELINE_FLAGS.contains(index))
            .collect();
        assert_eq!(set, flags, "{name}: flags");
        assert_eq!(changed, counts, "{name}: inventory counts");
        let selected = SELECTED
            .iter()
            .find(|(script, _)| *script == name)
            .map_or(selected_before, |(_, item)| Some(*item));
        assert_eq!(
            hud.inventory(None).unwrap().selected,
            selected,
            "{name}: selected item"
        );
    }
}
